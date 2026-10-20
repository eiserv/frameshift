use super::matcher::{Encoding, Matcher, Rules, Work, units};
use super::pattern::{ParseBounds, Parsed, Pattern, PatternRejection, glob_parent, parse};

const BOUNDS: ParseBounds = ParseBounds {
    max_bytes: 4096,
    max_alternatives: 256,
};

fn pattern(text: &str) -> Pattern {
    match parse(text, true, BOUNDS) {
        Ok(Parsed::Pattern(pattern)) => pattern,
        other => panic!("{text}: {other:?}"),
    }
}

fn rejection(text: &str) -> PatternRejection {
    parse(text, true, BOUNDS).expect_err(text)
}

fn rules(encoding: Encoding) -> Rules {
    Rules {
        encoding,
        dot: false,
        class_matches_leading_dot: false,
        trailing_globstar_nonempty: false,
        fold_case: false,
    }
}

fn matches(pattern_text: &str, path: &str, rules: Rules) -> bool {
    let matcher = Matcher::new(&pattern(pattern_text), rules);
    let path: Vec<Vec<u32>> = path.split('/').map(|name| units(name, rules)).collect();
    matcher.matches(&path, &mut Work::new(1 << 20)).unwrap()
}

#[test]
fn accepts_the_core_grammar() {
    for text in [
        "packages/*",
        "apps/**",
        "./packages/*",
        "packages/*/",
        "!packages/legacy",
        "{apps,packages}/*",
        "p/{a,{b,c}}",
        "p/[a-c]x",
        "p/[!a]*",
        "p/?x",
        ".config/tools",
        "@scope/*",
        "a b/~x",
        "\u{e4}/\u{1F600}",
    ] {
        pattern(text);
    }
    assert_eq!(parse(".", true, BOUNDS), Ok(Parsed::Root));
    assert_eq!(parse("./", true, BOUNDS), Ok(Parsed::Root));
    let negated = pattern("!./a/");
    assert!(negated.negated && negated.dot_slash && negated.trailing_slash);
    assert_eq!(&*negated.body, "a");
}

#[test]
fn rejects_what_matchers_read_differently() {
    use PatternRejection as R;
    for (text, expected) in [
        ("", R::Empty),
        ("/packages/*", R::Path),
        ("packages/../a", R::Path),
        ("a/./b", R::Path),
        ("a//b", R::EmptySegment),
        ("!!a", R::Negation),
        ("a/!b", R::Negation),
        ("a\\*", R::Character),
        ("#a", R::Character),
        ("a,b", R::Character),
        ("p/+(a|b)", R::Character),
        ("p/a+b", R::Character),
        ("p/<a:1>", R::Character),
        ("p/a$", R::Character),
        ("p/[^a]", R::Class),
        ("p/[[:alpha:]]", R::Class),
        ("p/[a-Z]", R::Class),
        ("p/[]", R::Class),
        ("p/{a}", R::Braces),
        ("p/{a,}", R::Braces),
        ("p/{1..3}", R::Braces),
        ("p/{*,a}", R::Braces),
        ("p/{.a,b}", R::Braces),
        ("p/a**", R::Globstar),
        ("p/***", R::Globstar),
        ("p/.*", R::DotWildcard),
    ] {
        assert_eq!(rejection(text), expected, "{text}");
    }
    let bounds = ParseBounds {
        max_bytes: 8,
        max_alternatives: 2,
    };
    assert_eq!(
        parse("abcdefghi", true, bounds),
        Err(PatternRejection::TooLong)
    );
    assert_eq!(
        parse("{a,b,c}", true, bounds),
        Err(PatternRejection::TooManyAlternatives)
    );
}

#[test]
fn yarn_classic_reads_a_leading_bang_literally() {
    let Ok(Parsed::Pattern(literal)) = parse("!packages/b", false, BOUNDS) else {
        panic!("not parsed");
    };
    assert!(!literal.negated);
    assert_eq!(literal.segments[0].literal().as_deref(), Some("!packages"));
}

#[test]
fn segments_follow_the_dot_rule_and_units() {
    let utf16 = rules(Encoding::Utf16);
    let scalar = rules(Encoding::Scalar);
    assert!(matches("p/*", "p/a", utf16));
    assert!(!matches("p/*", "p/.a", utf16));
    assert!(!matches("p/?a", "p/.a", utf16));
    assert!(!matches("p/[!x]a", "p/.a", utf16));
    let picomatch = Rules {
        class_matches_leading_dot: true,
        ..utf16
    };
    assert!(matches("p/[!x]a", "p/.a", picomatch));
    assert!(matches(".h/*", ".h/a", utf16));
    // A JavaScript `?` consumes one UTF-16 unit, a Rust one a scalar.
    assert!(!matches("p/?", "p/\u{1F600}", utf16));
    assert!(matches("p/??", "p/\u{1F600}", utf16));
    assert!(matches("p/?", "p/\u{1F600}", scalar));
    assert!(matches("p/a*b*c", "p/axxbyyc", scalar));
    assert!(!matches("p/a*b*c", "p/axxbyy", scalar));
    assert!(matches("p/[a-c]x", "p/bx", scalar));
    assert!(!matches("p/[!a-c]x", "p/bx", scalar));
}

#[test]
fn globstars_match_any_depth_but_not_dot_segments() {
    let rules = rules(Encoding::Scalar);
    assert!(matches("p/**", "p", rules));
    assert!(matches("p/**", "p/a/b", rules));
    assert!(!matches("p/**", "p/.a/b", rules));
    assert!(matches("**/x", "a/b/x", rules));
    assert!(matches("p/**/x", "p/x", rules));
    let nonempty = Rules {
        trailing_globstar_nonempty: true,
        ..rules
    };
    assert!(!matches("p/**", "p", nonempty));
    assert!(matches("p/**", "p/a", nonempty));
    let matcher = Matcher::new(&pattern("p/a/**"), rules);
    let path: Vec<Vec<u32>> = ["p"].iter().map(|name| units(name, rules)).collect();
    assert!(
        matcher
            .could_match_below(&path, &mut Work::new(1 << 20))
            .unwrap()
    );
}

#[test]
fn bun_negated_walks_invert_the_first_segment() {
    let rules = rules(Encoding::Scalar);
    let matcher = Matcher::negated_first(&pattern("!a*/x"), rules);
    let path =
        |text: &str| -> Vec<Vec<u32>> { text.split('/').map(|name| units(name, rules)).collect() };
    let mut work = Work::new(1 << 20);
    assert!(matcher.matches(&path("b/x"), &mut work).unwrap());
    assert!(!matcher.matches(&path("ab/x"), &mut work).unwrap());
    assert!(!matcher.matches(&path(".b/x"), &mut work).unwrap());
}

#[test]
fn brace_expansions_and_glob_parent() {
    let expanded = pattern("{a,b}/c{d,e}").expansions(16).unwrap();
    let texts: Vec<String> = expanded
        .iter()
        .map(|expansion| expansion.texts.join("/"))
        .collect();
    assert_eq!(texts, ["a/cd", "a/ce", "b/cd", "b/ce"]);
    assert!(pattern("{a,b}/{c,d}").expansions(3).is_none());
    assert_eq!(glob_parent(&["packages", "*"], false), 1);
    assert_eq!(glob_parent(&["packages", "a"], false), 1);
    assert_eq!(glob_parent(&["{a,b}", "**"], false), 0);
    // is-glob's strict check does not take a leading `?` as glob syntax.
    assert_eq!(glob_parent(&["?ib", "**"], false), 1);
    assert_eq!(glob_parent(&["a.?", "x"], false), 0);
    assert_eq!(glob_parent(&["?ib"], true), 1);
}

#[test]
fn matching_work_is_bounded() {
    let rules = rules(Encoding::Scalar);
    let matcher = Matcher::new(&pattern("**/*a*b*c*/**"), rules);
    let path: Vec<Vec<u32>> = (0..64).map(|_| units("aaaaaaaaaaaaaaaa", rules)).collect();
    assert!(matcher.matches(&path, &mut Work::new(100)).is_err());
}
