//! Fuzz targets and the contracts they check (`D-044`).
//!
//! A target runs one input through public engine entry points with default
//! limits and returns `Err` with a description when a documented contract
//! does not hold. Panics, aborts, and executions that do not finish are
//! detected by the supervisor. Inputs are arbitrary bytes: text that is not
//! UTF-8 must be rejected by source text validation and is not parsed.

use frameshift_corpus::check;
use frameshift_engine::{
    HtmlReferenceContext, HtmlReferenceValue, JsDialect, LexicalErrorKind, LexicalLimits,
    ParseLimits, RegExpFlag, SourceGoal, SourceText, TextError, TextLimits,
    decode_html_character_reference, decode_jsx_character_reference, scan_js_regexp,
    validate_js_regexp,
};

use crate::{
    astro, collections, css, edits, gitignore, hashes, identifiers, json, lexical, package, paths,
    seeds, semver, workspace, yaml, yarn,
};

/// A fuzzed engine entry point with its contracts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Complete source text in every dialect and source goal.
    Program,
    /// One expression from the start of the text in every dialect.
    Expression,
    /// Regular-expression literal scanning and validation.
    RegExp,
    /// The lexical scanners at every character boundary.
    Lexical,
    /// The JSX and HTML character reference decoders at every boundary.
    References,
    /// The span edit core with edits decoded from the input.
    Edits,
    /// JSX conversion and Astro printing of every JSX element.
    Astro,
    /// Canonical source and portable output path validation with explicit limits.
    Paths,
    /// Stable catalog identifiers and percent-encoded unit identity.
    Identifiers,
    /// SHA-256 streaming, bounded updates, and canonical digest parsing.
    Hashes,
    /// Key tables, dense result slots, and atomic budgets.
    Collections,
    /// `.gitignore` parsing, matching, and work charges.
    Gitignore,
    /// JSON and JSONC parsing against an independent model.
    Json,
    /// npm versions, ranges, specifiers, and `packageManager` values.
    Semver,
    /// The `package.json` and lockfile models on arbitrary JSON and JSONC.
    Package,
    /// Full YAML syntax, unexpanded graph references, and the JSON subset.
    Yaml,
    /// Yarn classic lockfile syntax and model, with a write and reparse
    /// round trip.
    Yarn,
    /// Workspace patterns of every modeled release over directory listings.
    Workspace,
    /// CSS tokenization in both scan contexts.
    Css,
}

/// Every dialect, in the order of `JsDialect`.
pub const DIALECTS: [JsDialect; 5] = [
    JsDialect::JavaScript,
    JsDialect::Jsx,
    JsDialect::TypeScript,
    JsDialect::Tsx,
    JsDialect::TypeScriptDeclaration,
];

/// Both source goals.
pub const GOALS: [SourceGoal; 2] = [SourceGoal::Script, SourceGoal::Module];

impl Target {
    /// Every target, in the order the documentation lists them.
    pub const ALL: [Self; 19] = [
        Self::Program,
        Self::Expression,
        Self::RegExp,
        Self::Lexical,
        Self::References,
        Self::Edits,
        Self::Astro,
        Self::Paths,
        Self::Identifiers,
        Self::Hashes,
        Self::Collections,
        Self::Gitignore,
        Self::Json,
        Self::Semver,
        Self::Package,
        Self::Yaml,
        Self::Yarn,
        Self::Workspace,
        Self::Css,
    ];

    /// Returns the name used on the command line and in directories.
    pub fn name(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Expression => "expression",
            Self::RegExp => "regexp",
            Self::Lexical => "lexical",
            Self::References => "references",
            Self::Edits => "edits",
            Self::Astro => "astro",
            Self::Paths => "paths",
            Self::Identifiers => "identifiers",
            Self::Hashes => "hashes",
            Self::Collections => "collections",
            Self::Gitignore => "gitignore",
            Self::Json => "json",
            Self::Semver => "semver",
            Self::Package => "package",
            Self::Yaml => "yaml",
            Self::Yarn => "yarn",
            Self::Workspace => "workspace",
            Self::Css => "css",
        }
    }

    /// Returns the target with this name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.name() == name)
    }

    /// Describes what the target runs and checks.
    pub fn summary(self) -> &'static str {
        match self {
            Self::Program => {
                "parse_js_program in all five dialects and both goals; tree, repetition, and empty-edit contracts"
            }
            Self::Expression => {
                "parse_js_expression from offset 0 in all five dialects; tree and repetition contracts"
            }
            Self::RegExp => {
                "scan_js_regexp and validate_js_regexp at offset 0; token spans, flags, and error locations"
            }
            Self::Lexical => {
                "trivia, string, numeric, punctuator, template, identifier, and JSX scanners at every boundary"
            }
            Self::References => {
                "JSX and HTML character reference decoders at every boundary; spans and agreement"
            }
            Self::Edits => {
                "apply_edits with decoded edits; provenance, accounting, and order independence"
            }
            Self::Astro => {
                "convert_jsx and print_astro_component on JSX and TSX modules; determinism, anchors, and provenance"
            }
            Self::Paths => {
                "source and output paths from arbitrary bytes; acceptance model, limits, spelling, and conversions"
            }
            Self::Identifiers => {
                "catalog grammar and unit identity; independent acceptance and encoding, limits, and repetition"
            }
            Self::Hashes => {
                "SHA-256 streaming, failed-update preservation, byte limits, and independent digest grammar"
            }
            Self::Collections => {
                "key tables, dense slots, and budgets against independent models; order independence and atomic failures"
            }
            Self::Gitignore => {
                ".gitignore parsing and matching against a recursive model; work charges, atomic budgets, and repetition"
            }
            Self::Json => {
                "parse_json in both dialects against a recursive model; values, duplicate keys, spans, rewriting, and limits"
            }
            Self::Semver => {
                "npm ranges, versions, specifiers, and packageManager; canonical text, precedence model, minimums"
            }
            Self::Package => {
                "package.json and npm and Bun lockfile models on JSON and JSONC; spans, sorting, and resolution"
            }
            Self::Yaml => {
                "YAML streams, spans, graph references, deterministic errors, bounds, agreement on the JSON subset, and the pnpm and Yarn Berry models"
            }
            Self::Yarn => {
                "Yarn classic lockfiles: spans, node bound, duplicate keys, the classic model, and a write and reparse round trip"
            }
            Self::Workspace => {
                "workspace patterns of every release over listings: determinism, order independence, listed members, required fields, rejections, and work bounds"
            }
            Self::Css => {
                "CSS tokenization: exact coverage, repetition, step-wise agreement in both contexts, values, parse error offsets, and the token limit"
            }
        }
    }

    /// Returns the default maximum input length. Targets that scan at every
    /// boundary use shorter inputs, because their work grows faster.
    pub fn max_len(self) -> usize {
        match self {
            Self::Program
            | Self::Expression
            | Self::Edits
            | Self::Astro
            | Self::Paths
            | Self::Identifiers
            | Self::Hashes
            | Self::Collections
            | Self::Package => 4_096,
            Self::Yaml | Self::Yarn | Self::Workspace | Self::Css => 1_024,
            Self::RegExp | Self::References | Self::Gitignore | Self::Json | Self::Semver => 1_024,
            Self::Lexical => 512,
        }
    }

    /// Returns the tokens that the mutator inserts for this target.
    pub fn dictionary(self) -> &'static [&'static str] {
        seeds::dictionary(self)
    }

    /// Returns the built-in fictional seed inputs.
    pub fn seeds(self) -> Vec<Vec<u8>> {
        seeds::inputs(self)
    }

    /// Runs one input and returns a description of the first violated
    /// contract.
    pub fn run(self, input: &[u8]) -> Result<(), String> {
        match self {
            Self::Program => program(input),
            Self::Expression => expression(input),
            Self::RegExp => regexp(input),
            Self::Lexical => lexical::run(input),
            Self::References => references(input),
            Self::Edits => edits::run(input),
            Self::Astro => astro::run(input),
            Self::Paths => paths::run(input),
            Self::Identifiers => identifiers::run(input),
            Self::Hashes => hashes::run(input),
            Self::Collections => collections::run(input),
            Self::Gitignore => gitignore::run(input),
            Self::Json => json::run(input),
            Self::Semver => semver::run(input),
            Self::Package => package::run(input),
            Self::Yaml => yaml::run(input),
            Self::Yarn => yarn::run(input),
            Self::Workspace => workspace::run(input),
            Self::Css => css::run(input),
        }
    }
}

/// Returns the input as text, or `None` once source text validation has
/// rejected input that is not UTF-8 as it must.
pub fn text(input: &[u8]) -> Result<Option<&str>, String> {
    match std::str::from_utf8(input) {
        Ok(text) => Ok(Some(text)),
        Err(error) => match SourceText::from_bytes(input.to_vec(), TextLimits::default()) {
            Err(TextError::InvalidUtf8 { valid_up_to }) if valid_up_to == error.valid_up_to() => {
                Ok(None)
            }
            Err(other) => Err(format!(
                "invalid UTF-8 up to byte {} was rejected as {other:?}",
                error.valid_up_to()
            )),
            Ok(_) => Err("source text accepted invalid UTF-8".to_owned()),
        },
    }
}

/// Creates source text that the target already knows to be valid.
pub fn source(text: &str) -> Result<SourceText, String> {
    SourceText::new(text.to_owned(), TextLimits::default())
        .map_err(|error| format!("source text: {error:?}"))
}

fn program(input: &[u8]) -> Result<(), String> {
    let Some(text) = text(input)? else {
        return Ok(());
    };
    for goal in GOALS {
        for dialect in DIALECTS {
            check::program(text, goal, dialect, ParseLimits::default())
                .map_err(|error| format!("{goal:?} {dialect:?}: {error}"))?;
        }
    }
    Ok(())
}

fn expression(input: &[u8]) -> Result<(), String> {
    let Some(text) = text(input)? else {
        return Ok(());
    };
    for dialect in DIALECTS {
        check::expression(text, 0, dialect, ParseLimits::default())
            .map_err(|error| format!("{dialect:?}: {error}"))?;
    }
    Ok(())
}

// The contracts of the generated test in `tests/regexp.rs`, at offset 0.
fn regexp(input: &[u8]) -> Result<(), String> {
    let Some(text) = text(input)? else {
        return Ok(());
    };
    let source = source(text)?;
    let limits = LexicalLimits::default();
    let scanned = scan_js_regexp(&source, 0, limits);
    let validated = validate_js_regexp(&source, 0, limits);
    if scanned != scan_js_regexp(&source, 0, limits)
        || validated != validate_js_regexp(&source, 0, limits)
    {
        return Err("a repeated scan or validation changed its result".to_owned());
    }
    match (scanned, validated) {
        (Ok(token), validated) => {
            let body = token.body_span;
            let flags = token.flags_span;
            if token.span.start() != 0
                || body.start() != 1
                || body.is_empty()
                || flags.start() != body.end() + 1
                || flags.end() != token.span.end()
                || source.slice(token.span).is_err()
                || text[..token.span.end()].contains(['\n', '\r', '\u{2028}', '\u{2029}'])
            {
                return Err(format!("inconsistent token {token:?}"));
            }
            match validated {
                Ok(literal) => {
                    let letters = &text[flags.start()..flags.end()];
                    let present = RegExpFlag::ALL
                        .iter()
                        .filter(|flag| literal.flags.contains(**flag))
                        .count();
                    if literal.token != token
                        || present != letters.chars().count()
                        || !RegExpFlag::ALL.iter().all(|flag| {
                            literal.flags.contains(*flag) == letters.contains(flag.letter())
                        })
                    {
                        return Err(format!(
                            "validated literal {literal:?} disagrees with {token:?}"
                        ));
                    }
                }
                Err(failure) => {
                    // A scanned literal can only fail flag or pattern
                    // validation.
                    let flag = matches!(
                        failure.kind,
                        LexicalErrorKind::InvalidRegExpFlag
                            | LexicalErrorKind::DuplicateRegExpFlag
                            | LexicalErrorKind::IncompatibleRegExpFlags
                    );
                    let (start, end) = if flag {
                        (flags.start(), flags.end().saturating_sub(1))
                    } else if matches!(failure.kind, LexicalErrorKind::InvalidRegExpPattern(_)) {
                        (body.start(), body.end())
                    } else {
                        return Err(format!("a scanned literal failed with {failure:?}"));
                    };
                    if !(start..=end).contains(&failure.offset) {
                        return Err(format!("{failure:?} lies outside {start}..={end}"));
                    }
                }
            }
        }
        (Err(failure), validated) => {
            if validated != Err(failure.clone()) || source.validate_offset(failure.offset).is_err()
            {
                return Err(format!(
                    "scan error {failure:?} but validation {validated:?}"
                ));
            }
        }
    }
    Ok(())
}

// Every JSX reference is an HTML reference with the same span and, outside
// the values that HTML replaces or that differ between the tables, the same
// value; an attribute reference is a text reference; and only an
// unterminated named reference before `=` or an ASCII alphanumeric differs
// between the contexts.
fn references(input: &[u8]) -> Result<(), String> {
    let Some(text) = text(input)? else {
        return Ok(());
    };
    for offset in (0..=text.len()).filter(|&offset| text.is_char_boundary(offset)) {
        let at = |context| format!("{context} at {offset}");
        let jsx = decode_jsx_character_reference(text, offset).map_err(|e| at(format!("{e:?}")))?;
        let html = decode_html_character_reference(text, offset, HtmlReferenceContext::Text)
            .map_err(|e| at(format!("{e:?}")))?;
        let attribute =
            decode_html_character_reference(text, offset, HtmlReferenceContext::Attribute)
                .map_err(|e| at(format!("{e:?}")))?;
        for reference in [html, attribute].into_iter().flatten() {
            let spelling = text.get(reference.span.start()..reference.span.end());
            if reference.span.start() != offset
                || reference.span.len() < 3
                || !spelling.is_some_and(|spelling| {
                    spelling.starts_with('&') && spelling.ends_with(';') == reference.terminated
                })
            {
                return Err(at(format!("inconsistent HTML reference {reference:?}")));
            }
        }
        match (html, attribute) {
            (_, Some(attribute)) if html != Some(attribute) => {
                return Err(at(format!("attribute {attribute:?} but text {html:?}")));
            }
            (Some(reference), None) => {
                let next = text[reference.span.end()..].bytes().next();
                if reference.terminated
                    || matches!(reference.value, HtmlReferenceValue::Numeric(_))
                    || !next.is_some_and(|byte| byte == b'=' || byte.is_ascii_alphanumeric())
                {
                    return Err(at(format!("text {reference:?} is literal in an attribute")));
                }
            }
            (_, _) => {}
        }
        let Some(jsx) = jsx else {
            continue;
        };
        let spelling = &text[jsx.span.start()..jsx.span.end()];
        let (Some(html), Some(attribute)) = (html, attribute) else {
            return Err(at(format!("JSX {jsx:?} is not an HTML reference")));
        };
        if jsx.span.start() != offset
            || !spelling.ends_with(';')
            || html.span != jsx.span
            || attribute.span != jsx.span
        {
            return Err(at(format!("JSX {jsx:?} disagrees with HTML {html:?}")));
        }
        let agrees = match (jsx.as_char(), html.value) {
            (Some(ch), HtmlReferenceValue::Numeric(value)) => {
                ch == value || ch == '\0' || ('\u{80}'..='\u{9f}').contains(&ch)
            }
            (Some(ch), HtmlReferenceValue::Named(value)) => {
                let mut encoded = [0; 4];
                value == ch.encode_utf8(&mut encoded)
                    || spelling == "&lang;"
                    || spelling == "&rang;"
            }
            // A surrogate code point is a numeric reference that HTML
            // replaces.
            (None, value) => matches!(value, HtmlReferenceValue::Numeric('\u{FFFD}')),
        };
        if !agrees {
            return Err(at(format!("JSX {jsx:?} decodes differently from {html:?}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for target in Target::ALL {
            assert_eq!(Target::parse(target.name()), Some(target));
        }
        assert_eq!(Target::parse("parser"), None);
    }

    #[test]
    fn every_seed_satisfies_its_target() {
        for target in Target::ALL {
            let seeds = target.seeds();
            assert!(seeds.len() >= 8, "{target:?}");
            for seed in seeds {
                assert!(seed.len() <= target.max_len(), "{target:?}");
                target.run(&seed).unwrap_or_else(|error| {
                    panic!("{target:?} {:?}: {error}", String::from_utf8_lossy(&seed))
                });
            }
        }
    }

    #[test]
    fn malformed_inputs_are_valid_outcomes() {
        for input in [
            &b""[..],
            b"\xff\xfe",
            b"const = ;",
            b"&#x110000;&amp",
            b"/[/",
            b"\xe2\x80",
            b"<a></b>",
        ] {
            for target in Target::ALL {
                target
                    .run(input)
                    .unwrap_or_else(|error| panic!("{target:?} {input:?}: {error}"));
            }
        }
    }
}
