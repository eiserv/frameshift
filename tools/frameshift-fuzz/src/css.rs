//! CSS tokenization contracts (`D-077`).
//!
//! Tokens and comments must cover the text exactly once after an optional
//! byte order mark, tokenization must repeat identically and agree with
//! step-wise parser-directed scanning in both contexts, every value must
//! decode, names without escapes or NUL must borrow their source, parse
//! errors must lie on character boundaries in source order, and the token
//! limit must be inclusive.

use frameshift_engine::foundation::Span;
use frameshift_engine::syntax::{
    CssError, CssLimits, CssNodeKind, CssParserError, CssParserLimits, CssTokenContext,
    CssTokenKind, CssTokens, css_dimension_unit, css_token_value, parse_css, scan_css_token,
    tokenize_css,
};

use crate::targets;

/// Checks coverage, repetition, step-wise agreement, values, errors, and
/// the limit.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = targets::text(input)? else {
        return Ok(());
    };
    let source = targets::source(text)?;
    let limits = CssLimits::default();
    let tokens = tokenize_css(&source, limits).map_err(|error| format!("tokenize: {error}"))?;
    if tokenize_css(&source, limits).as_ref() != Ok(&tokens) {
        return Err("tokenization is not repeatable".to_owned());
    }
    let start = if tokens.has_bom() { 3 } else { 0 };
    if tokens.has_bom() != text.starts_with('\u{FEFF}') {
        return Err("byte order mark misreported".to_owned());
    }
    check_partition(text, start, &tokens)?;
    check_steps(&source, start, &tokens)?;
    check_values(&source, text, &tokens)?;
    check_errors(text, &tokens)?;
    check_unicode_ranges(&source, start)?;
    check_parser(&source)?;
    let count = tokens.tokens().len() + tokens.comments().len();
    if tokenize_css(&source, CssLimits { max_tokens: count }).as_ref() != Ok(&tokens) {
        return Err("the token limit is not inclusive".to_owned());
    }
    if count > 0
        && tokenize_css(
            &source,
            CssLimits {
                max_tokens: count - 1,
            },
        ) != Err(CssError::TokenLimit)
    {
        return Err("a lower token limit was not enforced".to_owned());
    }
    Ok(())
}

fn check_parser(source: &frameshift_engine::foundation::SourceText) -> Result<(), String> {
    let limits = CssParserLimits::default();
    let parsed = parse_css(source, CssLimits::default(), limits);
    if parse_css(source, CssLimits::default(), limits) != parsed {
        return Err("CSS parsing is not repeatable".to_owned());
    }
    let doc = match parsed {
        Ok(doc) => doc,
        Err(CssParserError::DepthLimit | CssParserError::DiagnosticLimit) => return Ok(()),
        Err(
            error @ (CssParserError::NodeLimit
            | CssParserError::WorkLimit
            | CssParserError::Tokenization(_)),
        ) => {
            return Err(format!("unexpected CSS parse failure: {error}"));
        }
    };
    for component in doc.components() {
        if !doc.has_errors() {
            css_token_value(source, &component.token)
                .map_err(|error| format!("CSS parser component cannot be decoded: {error}"))?;
        }
        source
            .slice(component.span)
            .map_err(|error| format!("CSS component span: {error}"))?;
        let mut end = component.token.span.end();
        for &id in &component.values {
            let child = doc.component(id).ok_or("missing CSS component")?;
            if child.span.start() < end || child.span.end() > component.span.end() {
                return Err(format!(
                    "CSS child component spans are not ordered and contained: parent {:?}, child {:?}, previous end {end}",
                    component.span, child.span
                ));
            }
            end = child.span.end();
        }
        if let Some(closing) = component.closing
            && (closing.span.start() < end || closing.span.end() != component.span.end())
        {
            return Err("CSS component closer is outside its span".to_owned());
        }
    }
    for node in doc.nodes() {
        source
            .slice(node.span)
            .map_err(|error| format!("CSS syntax span: {error}"))?;
        if let CssNodeKind::Rule(rule) = &node.kind {
            let mut end = node.span.start();
            for &id in &rule.children {
                let child = doc.node(id).ok_or("missing CSS syntax child")?;
                if child.span.start() < end || child.span.end() > node.span.end() {
                    return Err("CSS syntax children are not ordered and contained".to_owned());
                }
                end = child.span.end();
            }
        }
    }
    for error in doc.diagnostics() {
        source
            .validate_offset(error.offset)
            .map_err(|error| format!("CSS diagnostic offset: {error}"))?;
    }
    if !doc
        .diagnostics()
        .windows(2)
        .all(|pair| pair[0].offset <= pair[1].offset)
    {
        return Err("CSS diagnostics are out of source order".to_owned());
    }
    let count = doc.components().len() + doc.nodes().len();
    if parse_css(
        source,
        CssLimits::default(),
        CssParserLimits {
            max_nodes: count,
            ..limits
        },
    )
    .as_ref()
        != Ok(&doc)
    {
        return Err("CSS parser node limit is not inclusive".to_owned());
    }
    if count > 0
        && parse_css(
            source,
            CssLimits::default(),
            CssParserLimits {
                max_nodes: count - 1,
                ..limits
            },
        ) != Err(CssParserError::NodeLimit)
    {
        return Err("CSS parser did not enforce its node limit".to_owned());
    }
    let count = doc.diagnostics().len();
    if parse_css(
        source,
        CssLimits::default(),
        CssParserLimits {
            max_diagnostics: count,
            ..limits
        },
    )
    .as_ref()
        != Ok(&doc)
    {
        return Err("CSS parser diagnostic limit is not inclusive".to_owned());
    }
    if count > 0
        && parse_css(
            source,
            CssLimits::default(),
            CssParserLimits {
                max_diagnostics: count - 1,
                ..limits
            },
        ) != Err(CssParserError::DiagnosticLimit)
    {
        return Err("CSS parser did not enforce its diagnostic limit".to_owned());
    }
    Ok(())
}

fn check_partition(text: &str, start: usize, tokens: &CssTokens) -> Result<(), String> {
    let mut spans: Vec<Span> = tokens.tokens().iter().map(|token| token.span).collect();
    spans.extend(tokens.comments());
    spans.sort();
    let mut offset = start;
    for span in spans {
        if span.start() != offset || span.is_empty() {
            return Err(format!("gap, overlap, or empty span at {offset}: {span:?}"));
        }
        if !text.is_char_boundary(span.end()) {
            return Err(format!("span {span:?} divides a character"));
        }
        offset = span.end();
    }
    if offset != text.len() {
        return Err(format!("tokens end at {offset} of {}", text.len()));
    }
    Ok(())
}

/// Scanning from the end of each token reproduces the same comments and
/// tokens, and scanning at a token's start reproduces the token.
fn check_steps(
    source: &frameshift_engine::foundation::SourceText,
    start: usize,
    tokens: &CssTokens,
) -> Result<(), String> {
    let mut offset = start;
    let mut seen_tokens = Vec::new();
    let mut seen_comments = Vec::new();
    let mut seen_errors = Vec::new();
    loop {
        let scan = scan_css_token(source, offset, CssTokenContext::default())
            .map_err(|error| format!("scan at {offset}: {error}"))?;
        seen_comments.extend(scan.comments);
        seen_errors.extend(scan.errors);
        let Some(token) = scan.token else {
            break;
        };
        let again = scan_css_token(source, token.span.start(), CssTokenContext::default())
            .map_err(|error| format!("rescan at {}: {error}", token.span.start()))?;
        if again.token != Some(token) || !again.comments.is_empty() {
            return Err(format!(
                "scanning at {} does not reproduce {token:?}",
                token.span.start()
            ));
        }
        offset = token.span.end();
        seen_tokens.push(token);
    }
    if seen_tokens != tokens.tokens()
        || seen_comments != tokens.comments()
        || seen_errors != tokens.errors()
    {
        return Err("step-wise scanning differs from tokenization".to_owned());
    }
    Ok(())
}

fn check_values(
    source: &frameshift_engine::foundation::SourceText,
    text: &str,
    tokens: &CssTokens,
) -> Result<(), String> {
    for token in tokens.tokens() {
        let value = css_token_value(source, token)
            .map_err(|error| format!("value of {token:?}: {error}"))?;
        let raw = &text[token.span.start()..token.span.end()];
        let plain = !raw.bytes().any(|byte| byte == b'\\' || byte == 0);
        let expected = match token.kind {
            CssTokenKind::Ident => Some(raw),
            CssTokenKind::Function => raw.strip_suffix('('),
            CssTokenKind::AtKeyword | CssTokenKind::Hash(_) => raw.get(1..),
            CssTokenKind::Delim(_)
            | CssTokenKind::String
            | CssTokenKind::BadString
            | CssTokenKind::Url
            | CssTokenKind::BadUrl
            | CssTokenKind::Number(_)
            | CssTokenKind::Percentage(_)
            | CssTokenKind::Dimension { .. }
            | CssTokenKind::UnicodeRange { .. }
            | CssTokenKind::Whitespace
            | CssTokenKind::Cdo
            | CssTokenKind::Cdc
            | CssTokenKind::Colon
            | CssTokenKind::Semicolon
            | CssTokenKind::Comma
            | CssTokenKind::OpenSquare
            | CssTokenKind::CloseSquare
            | CssTokenKind::OpenParen
            | CssTokenKind::CloseParen
            | CssTokenKind::OpenCurly
            | CssTokenKind::CloseCurly => None,
        };
        if plain
            && let Some(expected) = expected
            && value.as_deref() != Some(expected)
        {
            return Err(format!("{token:?} decodes to {value:?}, not its text"));
        }
        if let CssTokenKind::Dimension { number, unit } = token.kind {
            if number.span.start() != token.span.start()
                || number.span.end() != unit.start()
                || unit.end() != token.span.end()
            {
                return Err(format!("dimension parts do not cover {token:?}"));
            }
            css_dimension_unit(source, token)
                .map_err(|error| format!("unit of {token:?}: {error}"))?;
        }
        if let CssTokenKind::Number(number) = token.kind
            && number.span != token.span
        {
            return Err(format!("number text differs from {token:?}"));
        }
        if let CssTokenKind::Percentage(number) = token.kind
            && (number.span.start() != token.span.start()
                || number.span.end() + 1 != token.span.end())
        {
            return Err(format!("percentage text differs from {token:?}"));
        }
    }
    Ok(())
}

fn check_errors(text: &str, tokens: &CssTokens) -> Result<(), String> {
    let mut previous = 0;
    for error in tokens.errors() {
        if error.offset < previous
            || error.offset > text.len()
            || !text.is_char_boundary(error.offset)
        {
            return Err(format!(
                "parse error {error:?} out of order or off the text"
            ));
        }
        previous = error.offset;
    }
    Ok(())
}

/// With unicode ranges allowed, step-wise scanning still covers the text.
fn check_unicode_ranges(
    source: &frameshift_engine::foundation::SourceText,
    start: usize,
) -> Result<(), String> {
    let context = CssTokenContext {
        unicode_ranges_allowed: true,
    };
    let mut offset = start;
    loop {
        let scan = scan_css_token(source, offset, context)
            .map_err(|error| format!("range scan at {offset}: {error}"))?;
        let mut end = offset;
        for comment in &scan.comments {
            if comment.start() != end {
                return Err(format!("range scan comment gap at {end}"));
            }
            end = comment.end();
        }
        let Some(token) = scan.token else {
            return if end == source.len() {
                Ok(())
            } else {
                Err(format!("range scan stopped at {end}"))
            };
        };
        if token.span.start() != end || token.span.is_empty() {
            return Err(format!("range scan token gap at {end}"));
        }
        if let CssTokenKind::UnicodeRange { start, end } = token.kind
            && (start > 0xff_ffff || end > 0xff_ffff)
        {
            return Err(format!("range {start:x}-{end:x} beyond six hex digits"));
        }
        offset = token.span.end();
    }
}
