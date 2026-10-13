//! Validated names that printers embed in generated code and markup.
//!
//! Every name is checked when it is constructed, so a printer never has to
//! escape one and source content can never inject syntax through a name.

use std::fmt;

use frameshift_core::{SourceText, TextLimits};
use frameshift_syntax::{
    IdentifierKind, LexicalLimits, is_js_identifier_part, is_js_identifier_start,
    scan_js_identifier,
};

/// A JavaScript identifier that is valid as a binding in strict module code.
///
/// It has no escapes and is neither a reserved word, a strict-mode reserved
/// word, `await`, `eval`, nor `arguments`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsIdentifier(String);

impl JsIdentifier {
    /// Validates `name`, returning `None` when it is not a plain identifier
    /// that strict module code can bind.
    pub fn new(name: &str) -> Option<Self> {
        let mut chars = name.chars();
        let first = chars.next()?;
        if first == '\\' || !is_js_identifier_start(first) {
            return None;
        }
        if !chars.all(|ch| ch != '\\' && is_js_identifier_part(ch)) {
            return None;
        }
        if matches!(name, "eval" | "arguments") {
            return None;
        }
        // The scanner classifies reserved words with the parser's own table.
        let source = SourceText::new(name.to_owned(), TextLimits::default()).ok()?;
        let token = scan_js_identifier(&source, 0, LexicalLimits::default()).ok()?;
        let complete = token.span.end() == name.len();
        let plain = matches!(token.kind, IdentifierKind::Name) && !token.has_escape;
        (complete && plain && token.reserved_word.is_none()).then(|| Self(name.to_owned()))
    }

    /// Returns the identifier's spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JsIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of an HTML element: lowercase ASCII letters and digits, starting
/// with a letter, with single hyphens between parts as in custom elements.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HtmlTagName(String);

impl HtmlTagName {
    /// Validates `name`, returning `None` for any other spelling.
    pub fn new(name: &str) -> Option<Self> {
        is_lowercase_name(name).then(|| Self(name.to_owned()))
    }

    /// Returns the tag name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The tag name of an Astro or framework component: an identifier that
/// starts with an ASCII uppercase letter, or a dotted member path whose
/// parts are identifiers.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentName(String);

impl ComponentName {
    /// Validates `name`, returning `None` for a spelling that Astro would not
    /// treat as a component.
    pub fn new(name: &str) -> Option<Self> {
        let mut parts = name.split('.');
        let first = parts.next()?;
        JsIdentifier::new(first)?;
        let dotted = name.contains('.');
        if !dotted && !first.starts_with(|ch: char| ch.is_ascii_uppercase()) {
            return None;
        }
        // Later parts are property names, so reserved words are allowed.
        let valid_parts = parts.all(|part| {
            let mut chars = part.chars();
            chars.next().is_some_and(is_js_identifier_start) && chars.all(is_js_identifier_part)
        });
        valid_parts.then(|| Self(name.to_owned()))
    }

    /// Returns the component name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An attribute or directive name in an Astro template, such as `class`,
/// `data-id`, `client:load`, or `set:html`.
///
/// It starts with an ASCII letter or `_` and continues with ASCII letters,
/// digits, `_`, `-`, `.`, or `:`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttributeName(String);

impl AttributeName {
    /// Validates `name`, returning `None` for any other spelling.
    pub fn new(name: &str) -> Option<Self> {
        let mut bytes = name.bytes();
        let first = bytes.next()?;
        let valid = (first.is_ascii_alphabetic() || first == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"_-.:".contains(&byte));
        valid.then(|| Self(name.to_owned()))
    }

    /// Returns the attribute name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reports whether `name` is lowercase ASCII letters and digits starting with
/// a letter, optionally in hyphen-separated parts.
pub(crate) fn is_lowercase_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').enumerate().all(|(index, part)| {
            let mut bytes = part.bytes();
            match bytes.next() {
                Some(first) if index == 0 => {
                    first.is_ascii_lowercase()
                        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                }
                Some(first) => {
                    (first.is_ascii_lowercase() || first.is_ascii_digit())
                        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                }
                None => false,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_reserved_strict_and_escaped_names() {
        for valid in ["title", "_x", "$el", "caf\u{e9}", "Async", "of", "async"] {
            assert!(JsIdentifier::new(valid).is_some(), "{valid}");
        }
        for invalid in [
            "",
            "1a",
            "a-b",
            "class",
            "let",
            "static",
            "yield",
            "await",
            "eval",
            "arguments",
            "\\u0061",
            "a b",
            "#x",
        ] {
            assert!(JsIdentifier::new(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn tag_component_and_attribute_names_are_validated() {
        assert!(HtmlTagName::new("div").is_some());
        assert!(HtmlTagName::new("h1").is_some());
        assert!(HtmlTagName::new("my-element").is_some());
        for invalid in ["", "Div", "my--el", "-x", "x-", "1a", "svg:rect", "a b"] {
            assert!(HtmlTagName::new(invalid).is_none(), "{invalid}");
        }
        assert!(ComponentName::new("SiteHeader").is_some());
        assert!(ComponentName::new("ui.button").is_some());
        assert!(ComponentName::new("Menu.default").is_some());
        for invalid in ["", "header", "Menu.", ".Menu", "class.X", "A-B"] {
            assert!(ComponentName::new(invalid).is_none(), "{invalid}");
        }
        for valid in [
            "class",
            "data-id",
            "client:load",
            "set:html",
            "aria-label",
            "_x",
        ] {
            assert!(AttributeName::new(valid).is_some(), "{valid}");
        }
        for invalid in ["", "1x", "a b", "a=b", "a\"", "a/", "-x", "{x}"] {
            assert!(AttributeName::new(invalid).is_none(), "{invalid}");
        }
    }
}
