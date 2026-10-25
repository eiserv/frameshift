//! A minimal deterministic writer for Frameshift's versioned JSON documents.
//!
//! A document is built as a value tree with members in a fixed order and
//! printed with two-space indentation and LF line ends, ending with an LF, so
//! equal values print equal bytes on every platform. Strings are escaped as
//! RFC 8259 requires, with DEL, C1 controls, line and paragraph separators,
//! and bidirectional formatting characters escaped as well, and otherwise
//! kept as UTF-8. Numbers are nonnegative integers; documents carry no
//! floating-point values.

use std::fmt::Write as _;

/// A JSON value with members in insertion order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(&'static str, Value)>),
}

impl Value {
    pub(crate) fn string(text: &str) -> Self {
        Self::String(text.to_owned())
    }

    pub(crate) fn count(count: usize) -> Self {
        // `usize` is at most 64 bits on every supported target (limits.rs).
        Self::Number(count as u64)
    }

    pub(crate) fn optional<T>(value: Option<T>, convert: impl FnOnce(T) -> Self) -> Self {
        value.map_or(Self::Null, convert)
    }
}

/// Prints only if the exact serialized byte count fits the explicit bound.
pub(crate) fn print_bounded(value: &Value, max_bytes: usize) -> Option<String> {
    fn size(value: &Value, depth: usize) -> usize {
        match value {
            Value::Null => 4,
            Value::Bool(b) => {
                if *b {
                    4
                } else {
                    5
                }
            }
            Value::Number(n) => n.to_string().len(),
            Value::String(s) => string_size(s),
            Value::Array(a) => {
                if a.is_empty() {
                    2
                } else {
                    a.iter()
                        .fold(2usize.saturating_add(1 + depth * 2), |n, v| {
                            n.saturating_add(2 + (depth + 1) * 2)
                                .saturating_add(size(v, depth + 1))
                        })
                        .saturating_sub(1)
                }
            }
            Value::Object(o) => {
                if o.is_empty() {
                    2
                } else {
                    o.iter()
                        .fold(2usize.saturating_add(1 + depth * 2), |n, (key, v)| {
                            n.saturating_add(4 + (depth + 1) * 2)
                                .saturating_add(string_size(key))
                                .saturating_add(size(v, depth + 1))
                        })
                        .saturating_sub(1)
                }
            }
        }
    }
    (size(value, 0).checked_add(1)? <= max_bytes).then(|| print(value))
}

/// Prints a document followed by an LF.
pub(crate) fn print(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out.push('\n');
    out
}

fn write_value(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => {
            let _ = write!(out, "{number}");
        }
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_value(out, item, depth + 1);
            }
            newline(out, depth);
            out.push(']');
        }
        Value::Object(members) => {
            if members.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (index, (key, item)) in members.iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_string(out, key);
                out.push_str(": ");
                write_value(out, item, depth + 1);
            }
            newline(out, depth);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

pub(crate) fn string_size(text: &str) -> usize {
    text.chars().fold(2usize, |n, c| {
        n.saturating_add(match c {
            '"' | '\\' | '\u{08}' | '\u{0c}' | '\n' | '\r' | '\t' => 2,
            '\u{00}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => 6,
            c => c.len_utf8(),
        })
    })
}

pub(crate) fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for scalar in text.chars() {
        match scalar {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // C0 controls as RFC 8259 requires; DEL, C1 controls, line and
            // paragraph separators, and bidirectional formatting characters
            // as well, so that a document shown in a terminal can neither
            // start an escape sequence nor reorder the text around it.
            '\u{00}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => {
                let _ = write!(out, "\\u{:04x}", scalar as u32);
            }
            _ => out.push(scalar),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_size_bounds_include_indentation_unicode_and_escapes() {
        let value = Value::Object(vec![(
            "items",
            Value::Array(vec![
                Value::Null,
                Value::Bool(true),
                Value::Bool(false),
                Value::Number(u64::MAX),
                Value::String("a\n\t\\\"💫\u{0080}\u{202e}".into()),
                Value::Object(Vec::new()),
                Value::Array(Vec::new()),
            ]),
        )]);
        let printed = print(&value);
        assert_eq!(print_bounded(&value, printed.len()), Some(printed.clone()));
        assert_eq!(print_bounded(&value, printed.len() - 1), None);
    }

    #[test]
    fn values_print_deterministically_with_escapes_for_terminals() {
        let controls: String = ('\u{00}'..='\u{1f}').collect();
        let document = Value::Object(vec![
            ("kind", Value::string("test")),
            ("empty", Value::Array(Vec::new())),
            ("none", Value::Object(Vec::new())),
            (
                "items",
                Value::Array(vec![Value::Null, Value::Bool(true), Value::Number(7)]),
            ),
            (
                "text",
                Value::String(format!(
                    "{controls}\"\\é😀\u{7f}\u{9b}\u{61c}\u{200e}\u{2028}\u{202e}\u{2069}"
                )),
            ),
        ]);
        assert_eq!(
            print(&document),
            concat!(
                "{\n",
                "  \"kind\": \"test\",\n",
                "  \"empty\": [],\n",
                "  \"none\": {},\n",
                "  \"items\": [\n    null,\n    true,\n    7\n  ],\n",
                "  \"text\": \"\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007",
                "\\b\\t\\n\\u000b\\f\\r\\u000e\\u000f",
                "\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017",
                "\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f",
                "\\\"\\\\é😀\\u007f\\u009b\\u061c\\u200e\\u2028\\u202e\\u2069\"\n",
                "}\n"
            )
        );
    }
}
