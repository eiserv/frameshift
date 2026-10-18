//! `.gitignore` pattern contracts through the public engine facade (`D-065`).
//!
//! The table's expected decisions were produced by `git check-ignore`
//! (Git 2.52.0, `core.ignorecase=false`, no global excludes) on the same
//! files and paths outside the repository. A path is ignored when the last
//! matching pattern ignores it or one of its parent directories, as Git
//! applies patterns while it walks.

use frameshift_engine::foundation::{Budget, BudgetExceeded};
use frameshift_engine::input::{GitignoreFile, IgnoreDecision, IgnoreLimits};

use IgnoreDecision::{Ignored, Included};
use Kind::{Dir, File};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
}

/// Git's verdict: a parent directory that is ignored decides first.
fn verdict(file: &GitignoreFile, path: &str, kind: Kind) -> Option<IgnoreDecision> {
    let mut work = Budget::new(u64::MAX);
    let mut end = 0;
    while let Some(slash) = path[end..].find('/') {
        end += slash;
        if file.decide(&path[..end], true, &mut work).unwrap() == Some(Ignored) {
            return Some(Ignored);
        }
        end += 1;
    }
    file.decide(path, kind == Dir, &mut work).unwrap()
}

#[rustfmt::skip]
const GIT: &[(&str, Kind, &str, Option<IgnoreDecision>)] = &[
    ("**/foo", File, "foo", Some(Ignored)),
    ("**/foo", File, "a/foo", Some(Ignored)),
    ("**/foo", File, "a/b/foo", Some(Ignored)),
    ("**/foo", File, "xfoo", None),
    ("foo/**", Dir, "foo", None),
    ("foo/**", File, "foo/a", Some(Ignored)),
    ("foo/**", File, "foo/a/b", Some(Ignored)),
    ("a/**/b", File, "a/b", Some(Ignored)),
    ("a/**/b", File, "a/x/b", Some(Ignored)),
    ("a/**/b", File, "a/x/y/b", Some(Ignored)),
    ("a/**/b", File, "ab", None),
    ("a/**/b", File, "a/xb", None),
    ("a**b", File, "ab", Some(Ignored)),
    ("a**b", File, "axxb", Some(Ignored)),
    ("a**b", File, "a/b", None),
    ("x/a**b", File, "x/ab", Some(Ignored)),
    ("x/a**b", File, "x/a/b", None),
    ("x/a**b", File, "x/axb", Some(Ignored)),
    ("**a", File, "a", Some(Ignored)),
    ("**a", File, "ba", Some(Ignored)),
    ("**a", File, "b/a", Some(Ignored)),
    ("x/**b", File, "x/b", Some(Ignored)),
    ("x/**b", File, "x/ab", Some(Ignored)),
    ("x/**b", File, "x/a/b", None),
    ("**\\/x", File, "x", None),
    ("**\\/x", File, "a/x", Some(Ignored)),
    ("**\\/x", File, "a/b/x", Some(Ignored)),
    ("*", File, "a", Some(Ignored)),
    ("*", File, "a/b", Some(Ignored)),
    ("/*.js", File, "a.js", Some(Ignored)),
    ("/*.js", File, "d/a.js", None),
    ("*.js", File, "a.js", Some(Ignored)),
    ("*.js", File, "d/a.js", Some(Ignored)),
    ("doc/*.txt", File, "doc/a.txt", Some(Ignored)),
    ("doc/*.txt", File, "doc/x/a.txt", None),
    ("doc/*.txt", File, "a/doc/a.txt", None),
    ("[a-c]x", File, "ax", Some(Ignored)),
    ("[a-c]x", File, "bx", Some(Ignored)),
    ("[a-c]x", File, "dx", None),
    ("[!a]x", File, "ax", None),
    ("[!a]x", File, "bx", Some(Ignored)),
    ("[^a]x", File, "ax", None),
    ("[^a]x", File, "bx", Some(Ignored)),
    ("[]a]x", File, "]x", Some(Ignored)),
    ("[]a]x", File, "ax", Some(Ignored)),
    ("[]a]x", File, "bx", None),
    ("[a-]x", File, "-x", Some(Ignored)),
    ("[a-]x", File, "ax", Some(Ignored)),
    ("[a-]x", File, "bx", None),
    ("[[:alpha:]]x", File, "ax", Some(Ignored)),
    ("[[:alpha:]]x", File, "Zx", Some(Ignored)),
    ("[[:alpha:]]x", File, "1x", None),
    ("[[:alpha:]]x", File, "\u{e9}x", None),
    ("[[:digit:][:upper:]]", File, "1", Some(Ignored)),
    ("[[:digit:][:upper:]]", File, "A", Some(Ignored)),
    ("[[:digit:][:upper:]]", File, "a", None),
    ("[[:space:]]", File, " ", Some(Ignored)),
    ("[[:space:]]", File, "\t", Some(Ignored)),
    ("[[:space:]]", File, "\u{b}", None),
    ("[[:space:]]", File, "\u{c}", None),
    ("[[:space:]]", File, "\r", Some(Ignored)),
    ("[[:punct:]]", File, "!", Some(Ignored)),
    ("[[:punct:]]", File, "]", Some(Ignored)),
    ("[[:punct:]]", File, "a", None),
    ("[[:foo:]]x", File, "fx", None),
    ("[[:foo:]]x", File, "x", None),
    ("[ax", File, "ax", None),
    ("[ax", File, "[ax", None),
    ("x/[[:a]", File, "x/[", Some(Ignored)),
    ("x/[[:a]", File, "x/:", Some(Ignored)),
    ("x/[[:a]", File, "x/a", Some(Ignored)),
    ("x/[[:a]", File, "x/b", None),
    ("[b-a]", File, "a", None),
    ("[b-a]", File, "b", Some(Ignored)),
    ("a\\", File, "a", None),
    ("a\\", File, "a\\", None),
    ("\\#a", File, "#a", Some(Ignored)),
    ("\\!a", File, "!a", Some(Ignored)),
    ("a\\ ", File, "a ", Some(Ignored)),
    ("a\\ ", File, "a", None),
    ("a  ", File, "a", Some(Ignored)),
    ("a  ", File, "a ", None),
    ("#a", File, "#a", None),
    ("a\n!a", File, "a", Some(Included)),
    ("!a\na", File, "a", Some(Ignored)),
    ("a/", File, "a", None),
    ("a/", Dir, "a", Some(Ignored)),
    ("a/", File, "b/a", None),
    ("a/", Dir, "b/a", Some(Ignored)),
    ("/a/", Dir, "a", Some(Ignored)),
    ("/a/", Dir, "b/a", None),
    ("a\r\nb\r\n", File, "a", Some(Ignored)),
    ("a\r\nb\r\n", File, "b", Some(Ignored)),
    ("a\r\nb\r\n", File, "a\r", None),
    ("\u{feff}a\n", File, "a", Some(Ignored)),
    ("x/a?c", File, "x/abc", Some(Ignored)),
    ("x/a?c", File, "x/a/c", None),
    ("x/a[/]c", File, "x/a/c", None),
    ("x/*", File, "x/a", Some(Ignored)),
    ("x/*", Dir, "x/a", Some(Ignored)),
    ("x/*", File, "x/a/b", Some(Ignored)),
    ("x/*/y", File, "x/a/y", Some(Ignored)),
    ("x/*/y", File, "x/y", None),
    ("x/*/y", File, "x/a/b/y", None),
    ("x//y", File, "x/y", None),
    ("a.b", File, "a.b", Some(Ignored)),
    ("a.b", File, "aab", None),
    ("\u{e9}", File, "\u{e9}", Some(Ignored)),
    ("\u{e9}", File, "e\u{301}", None),
    ("caf?", File, "caf\u{e9}", None),
    ("caf?", File, "cafe", Some(Ignored)),
    ("/", File, "a", None),
    ("!", File, "a", None),
    ("A", File, "a", None),
    ("A", File, "A", Some(Ignored)),
    ("x/", File, "x/y", Some(Ignored)),
    ("x/", Dir, "x", Some(Ignored)),
    ("*/", Dir, "a", Some(Ignored)),
    ("*/", File, "a", None),
    ("*/", Dir, "a/b", Some(Ignored)),
    ("/**", File, "a", Some(Ignored)),
    ("/**", File, "a/b", Some(Ignored)),
    ("a/**/", Dir, "a/b", Some(Ignored)),
    ("a/**/", File, "a/b", None),
    ("a/**/", Dir, "a", None),
];

#[test]
fn decisions_match_git_check_ignore() {
    for &(content, kind, path, expected) in GIT {
        let file = GitignoreFile::parse(content.as_bytes()).unwrap();
        assert_eq!(
            verdict(&file, path, kind),
            expected,
            "{content:?} {kind:?} {path:?}"
        );
    }
}

#[test]
fn parsing_counts_only_patterns_that_can_match() {
    let content = b"\xEF\xBB\xBF# comment\n\n   \n!\n/\n//\na\r\n\\#b\n  c  \n[x\nd\\";
    let file = GitignoreFile::parse(content).unwrap();
    // `a`, `\#b`, `  c`, `[x`, and `d\` are patterns; the last two match nothing.
    assert_eq!(file.len(), 5);
    assert!(!file.is_empty());
    assert!(GitignoreFile::parse(b"# only\n\n").unwrap().is_empty());
    assert_eq!(GitignoreFile::parse(b"").unwrap().len(), 0);
    // A NUL ends the line.
    let file = GitignoreFile::parse(b"ab\0cd\n").unwrap();
    assert_eq!(verdict(&file, "ab", File), Some(Ignored));
    assert_eq!(verdict(&file, "abcd", File), None);
    // Arbitrary bytes are a valid file.
    let file = GitignoreFile::parse(b"\xff\xfe*\n").unwrap();
    assert_eq!(file.len(), 1);
    assert_eq!(verdict(&file, "x", File), None);
}

#[test]
fn the_last_matching_pattern_decides() {
    let file = GitignoreFile::parse(b"*.log\n!keep.log\nkeep.log\n!keep.log\n").unwrap();
    assert_eq!(verdict(&file, "a.log", File), Some(Ignored));
    assert_eq!(verdict(&file, "keep.log", File), Some(Included));
    assert_eq!(verdict(&file, "keep.txt", File), None);
}

#[test]
fn work_is_charged_per_matched_pattern_and_bounds_the_decision() {
    // Patterns are tried from the last; `dir/` is skipped for a file without a charge.
    let file = GitignoreFile::parse(b"abc\ndir/\nx/y\n").unwrap();
    let mut work = Budget::new(u64::MAX);
    assert_eq!(file.decide("p/q", false, &mut work), Ok(None));
    // `x/y`: (3 + 1) * (3 + 1); `abc` against `q`: (3 + 1) * (1 + 1).
    assert_eq!(work.used(), 16 + 8);
    let mut work = Budget::new(16 + 7);
    let error: BudgetExceeded = file.decide("p/q", false, &mut work).unwrap_err();
    assert_eq!(error.limit(), 16 + 7);
    assert_eq!(work.used(), 16);
    let mut empty = Budget::new(0);
    assert_eq!(
        GitignoreFile::parse(b"")
            .unwrap()
            .decide("a", false, &mut empty),
        Ok(None)
    );
}

#[test]
fn hostile_patterns_match_in_bounded_work() {
    let pattern = "*a".repeat(2_000);
    let file = GitignoreFile::parse(pattern.as_bytes()).unwrap();
    let subject = "a".repeat(1_999);
    let mut work = Budget::new(u64::MAX);
    assert_eq!(file.decide(&subject, false, &mut work), Ok(None));
    assert_eq!(work.used(), 4_001 * 2_000);
    let nested = format!("x/{}", "**/a/".repeat(500));
    let file = GitignoreFile::parse(nested.as_bytes()).unwrap();
    let deep = format!("x/{}", "a/".repeat(300));
    let mut work = Budget::new(u64::MAX);
    assert_eq!(
        file.decide(deep.trim_end_matches('/'), true, &mut work),
        Ok(None)
    );
}

#[test]
fn ignore_limits_have_documented_defaults() {
    let limits = IgnoreLimits::default();
    assert_eq!(limits.max_total_bytes, 4 * 1024 * 1024);
    assert_eq!(limits.max_match_work, 1 << 34);
}
