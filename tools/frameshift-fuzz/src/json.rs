//! JSON and JSONC parsing against an independent model (`D-068`).
//!
//! The model is a recursive-descent reader over decoded characters, a
//! different technique from the product's iterative byte parser. Both
//! dialects must agree with it on acceptance and on every decoded value,
//! duplicate-key mark, and lookup. Accepted documents must also survive a
//! canonical rewrite, and the depth and node limits must be inclusive.

use frameshift_engine::foundation::{SourceText, Span};
use frameshift_engine::syntax::{
    JsonDialect, JsonDocument, JsonErrorKind, JsonId, JsonLimits, JsonValue, parse_json,
};

use crate::targets;

/// Checks agreement with the model, spans, repetition, rewriting, and limits.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = targets::text(input)? else {
        return Ok(());
    };
    let source = targets::source(text)?;
    let limits = JsonLimits::default();
    let mut strict = None;
    for dialect in [JsonDialect::Json, JsonDialect::Jsonc] {
        let parsed = parse_json(&source, dialect, limits);
        if parse_json(&source, dialect, limits) != parsed {
            return Err(format!("{dialect:?}: parsing is not repeatable"));
        }
        let model = Model::parse(text, dialect == JsonDialect::Jsonc, limits.max_depth);
        match (&parsed, &model) {
            (Ok(document), Some(value)) => {
                check_spans(&source, document).map_err(|error| format!("{dialect:?}: {error}"))?;
                compare(document, document.root(), value)
                    .map_err(|error| format!("{dialect:?}: {error}"))?;
                check_duplicates(document, value)?;
                check_limits(&source, dialect, document, value)?;
                check_rewrite(value)?;
            }
            (Err(error), None) => {
                let span = error.span();
                if span.end() > source.len()
                    || !text.is_char_boundary(span.start())
                    || !text.is_char_boundary(span.end())
                {
                    return Err(format!("{dialect:?}: error span {span:?} off the text"));
                }
            }
            (Ok(_), None) => return Err(format!("{dialect:?}: accepted, model rejects")),
            (Err(error), Some(_)) => {
                return Err(format!("{dialect:?}: rejected ({error}), model accepts"));
            }
        }
        match dialect {
            JsonDialect::Json => strict = model,
            JsonDialect::Jsonc => {
                if strict.is_some() && strict != model {
                    return Err("JSONC changed a JSON document".to_owned());
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    fn depth(&self) -> usize {
        match self {
            Self::Null | Self::Bool(_) | Self::Number(_) | Self::String(_) => 0,
            Self::Array(items) => 1 + items.iter().map(Self::depth).max().unwrap_or(0),
            Self::Object(members) => {
                1 + members
                    .iter()
                    .map(|(_, value)| value.depth())
                    .max()
                    .unwrap_or(0)
            }
        }
    }

    /// Values plus keys, the product's node count.
    fn nodes(&self) -> usize {
        match self {
            Self::Null | Self::Bool(_) | Self::Number(_) | Self::String(_) => 1,
            Self::Array(items) => 1 + items.iter().map(Self::nodes).sum::<usize>(),
            Self::Object(members) => {
                1 + members
                    .iter()
                    .map(|(_, value)| 1 + value.nodes())
                    .sum::<usize>()
            }
        }
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Number(text) => out.push_str(text),
            Self::String(text) => write_string(text, out),
            Self::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Self::Object(members) => {
                out.push('{');
                for (index, (key, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch if u32::from(ch) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// The model reader. It returns `None` for every rejected document.
struct Model {
    chars: Vec<char>,
    at: usize,
    jsonc: bool,
    max_depth: usize,
}

impl Model {
    fn parse(text: &str, jsonc: bool, max_depth: usize) -> Option<Value> {
        let mut chars: Vec<char> = text.chars().collect();
        if chars.first() == Some(&'\u{feff}') {
            chars.remove(0);
        }
        let mut model = Self {
            chars,
            at: 0,
            jsonc,
            max_depth,
        };
        let value = model.value(0)?;
        model.space()?;
        (model.at == model.chars.len()).then_some(value)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, expected: char) -> bool {
        let found = self.peek() == Some(expected);
        if found {
            self.at += 1;
        }
        found
    }

    fn space(&mut self) -> Option<()> {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\n' | '\r') => self.at += 1,
                Some('/') if self.jsonc => match self.chars.get(self.at + 1) {
                    Some('/') => {
                        while !matches!(self.peek(), None | Some('\n' | '\r')) {
                            self.at += 1;
                        }
                    }
                    Some('*') => {
                        self.at += 2;
                        loop {
                            if self.peek()? == '*' && self.chars.get(self.at + 1) == Some(&'/') {
                                self.at += 2;
                                break;
                            }
                            self.at += 1;
                        }
                    }
                    _ => return Some(()),
                },
                _ => return Some(()),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        self.space()?;
        match self.peek()? {
            '[' => {
                if depth >= self.max_depth {
                    return None;
                }
                self.at += 1;
                let mut items = Vec::new();
                self.space()?;
                if self.eat(']') {
                    return Some(Value::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space()?;
                    if self.eat(']') {
                        return Some(Value::Array(items));
                    }
                    if !self.eat(',') {
                        return None;
                    }
                    self.space()?;
                    if self.jsonc && self.eat(']') {
                        return Some(Value::Array(items));
                    }
                }
            }
            '{' => {
                if depth >= self.max_depth {
                    return None;
                }
                self.at += 1;
                let mut members = Vec::new();
                self.space()?;
                if self.eat('}') {
                    return Some(Value::Object(members));
                }
                loop {
                    self.space()?;
                    if self.peek()? != '"' {
                        return None;
                    }
                    let key = self.string()?;
                    self.space()?;
                    if !self.eat(':') {
                        return None;
                    }
                    let value = self.value(depth + 1)?;
                    members.push((key, value));
                    self.space()?;
                    if self.eat('}') {
                        return Some(Value::Object(members));
                    }
                    if !self.eat(',') {
                        return None;
                    }
                    self.space()?;
                    if self.jsonc && self.eat('}') {
                        return Some(Value::Object(members));
                    }
                }
            }
            '"' => self.string().map(Value::String),
            '-' | '0'..='9' => self.number(),
            _ => {
                for (word, value) in [
                    ("true", Value::Bool(true)),
                    ("false", Value::Bool(false)),
                    ("null", Value::Null),
                ] {
                    let end = self.at + word.len();
                    if self
                        .chars
                        .get(self.at..end)
                        .is_some_and(|chars| chars.iter().copied().eq(word.chars()))
                    {
                        self.at = end;
                        return Some(value);
                    }
                }
                None
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.at;
        self.eat('-');
        if self.eat('0') {
            if self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                return None;
            }
        } else if self.digits() == 0 {
            return None;
        }
        if self.eat('.') && self.digits() == 0 {
            return None;
        }
        if self.eat('e') || self.eat('E') {
            if !self.eat('+') {
                self.eat('-');
            }
            if self.digits() == 0 {
                return None;
            }
        }
        Some(Value::Number(self.chars[start..self.at].iter().collect()))
    }

    fn unit(&mut self) -> Option<u32> {
        let mut value = 0;
        for _ in 0..4 {
            value = value * 16 + self.peek()?.to_digit(16)?;
            self.at += 1;
        }
        Some(value)
    }

    fn string(&mut self) -> Option<String> {
        self.at += 1;
        let mut text = String::new();
        loop {
            let ch = self.peek()?;
            self.at += 1;
            match ch {
                '"' => return Some(text),
                '\\' => {
                    let escape = self.peek()?;
                    self.at += 1;
                    text.push(match escape {
                        '"' => '"',
                        '\\' => '\\',
                        '/' => '/',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'u' => {
                            let unit = self.unit()?;
                            if (0xDC00..0xE000).contains(&unit) {
                                return None;
                            }
                            if (0xD800..0xDC00).contains(&unit) {
                                if !(self.eat('\\') && self.eat('u')) {
                                    return None;
                                }
                                let low = self.unit()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return None;
                                }
                                char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))?
                            } else {
                                char::from_u32(unit)?
                            }
                        }
                        _ => return None,
                    });
                }
                ch if u32::from(ch) < 0x20 => return None,
                ch => text.push(ch),
            }
        }
    }
}

fn compare(document: &JsonDocument<'_>, id: JsonId, expected: &Value) -> Result<(), String> {
    let actual = document.value(id).ok_or("missing node")?;
    match (actual, expected) {
        (JsonValue::Null, Value::Null) => Ok(()),
        (JsonValue::Bool(left), Value::Bool(right)) if left == *right => Ok(()),
        (JsonValue::Number(left), Value::Number(right)) if left.text() == right => {
            let integer = !right.contains(['.', 'e', 'E']);
            let unsigned = (integer && !right.starts_with('-'))
                .then(|| right.parse::<u64>().ok())
                .flatten();
            let signed = integer.then(|| right.parse::<i64>().ok()).flatten();
            if left.is_integer() != integer || left.as_u64() != unsigned || left.as_i64() != signed
            {
                return Err(format!("number conversions of {right}"));
            }
            Ok(())
        }
        (JsonValue::String(left), Value::String(right)) if left == right => Ok(()),
        (JsonValue::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (item, value) in left.iter().zip(right) {
                compare(document, item, value)?;
            }
            Ok(())
        }
        (JsonValue::Object(left), Value::Object(right)) if left.len() == right.len() => {
            for (index, (member, (key, value))) in left.members().zip(right).enumerate() {
                if member.key() != key {
                    return Err(format!("key {:?}, model {key:?}", member.key()));
                }
                let shadowed = right[index + 1..].iter().any(|(later, _)| later == key);
                if member.is_shadowed() != shadowed {
                    return Err(format!("key {key:?}: shadowed mark differs"));
                }
                compare(document, member.value(), value)?;
            }
            for (key, value) in right {
                let last = right
                    .iter()
                    .rev()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value);
                let found = left
                    .get(key)
                    .ok_or_else(|| format!("lookup of {key:?} failed"))?;
                compare(document, found, last.unwrap_or(value))?;
            }
            Ok(())
        }
        (actual, expected) => Err(format!("{actual:?}, model {expected:?}")),
    }
}

/// Spans lie on the text at character boundaries, ascend with identities,
/// and nest inside their containers.
fn check_spans(source: &SourceText, document: &JsonDocument<'_>) -> Result<(), String> {
    let text = source.as_str();
    let mut previous: Option<Span> = None;
    for id in document.ids() {
        let span = document.span(id).ok_or("missing span")?;
        if span.is_empty()
            || span.end() > text.len()
            || !text.is_char_boundary(span.start())
            || !text.is_char_boundary(span.end())
        {
            return Err(format!("span {span:?} off the text"));
        }
        if previous.is_some_and(|previous| previous.start() >= span.start()) {
            return Err(format!("span {span:?} does not ascend"));
        }
        previous = Some(span);
        let children: Vec<JsonId> = match document.value(id).ok_or("missing value")? {
            JsonValue::Array(array) => array.iter().collect(),
            JsonValue::Object(object) => object
                .members()
                .flat_map(|member| [member.key_id(), member.value()])
                .collect(),
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {
                Vec::new()
            }
        };
        let mut last = span.start();
        for child in children {
            let inner = document.span(child).ok_or("missing child span")?;
            if inner.start() <= last || inner.end() >= span.end() {
                return Err(format!("child {inner:?} outside {span:?}"));
            }
            last = inner.end() - 1;
        }
    }
    Ok(())
}

fn check_duplicates(document: &JsonDocument<'_>, value: &Value) -> Result<(), String> {
    fn collect(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
            Value::Array(items) => items.iter().for_each(|item| collect(item, out)),
            Value::Object(members) => {
                for (index, (key, value)) in members.iter().enumerate() {
                    if members[index + 1..].iter().any(|(later, _)| later == key) {
                        out.push(key.clone());
                    }
                    collect(value, out);
                }
            }
        }
    }
    let mut expected = Vec::new();
    collect(value, &mut expected);
    let listed: Vec<&str> = document
        .duplicate_keys()
        .map(|member| member.key())
        .collect();
    let starts: Vec<usize> = document
        .duplicate_keys()
        .map(|member| member.key_span().start())
        .collect();
    if listed != expected || starts.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!("duplicate keys {listed:?}, model {expected:?}"));
    }
    Ok(())
}

fn check_limits(
    source: &SourceText,
    dialect: JsonDialect,
    document: &JsonDocument<'_>,
    value: &Value,
) -> Result<(), String> {
    let nodes = value.nodes();
    if document.node_count() != nodes {
        return Err(format!("{} nodes, model {nodes}", document.node_count()));
    }
    let depth = value.depth();
    let exact = JsonLimits {
        max_depth: depth,
        max_nodes: nodes,
    };
    parse_json(source, dialect, exact).map_err(|error| format!("exact limits: {error}"))?;
    if depth > 0 {
        let shallow = JsonLimits {
            max_depth: depth - 1,
            ..exact
        };
        match parse_json(source, dialect, shallow)
            .map(|_| ())
            .map_err(|error| error.kind())
        {
            Err(JsonErrorKind::DepthLimit { limit }) if limit == depth - 1 => {}
            other => return Err(format!("depth limit {}: {other:?}", depth - 1)),
        }
    }
    let small = JsonLimits {
        max_nodes: nodes - 1,
        ..exact
    };
    match parse_json(source, dialect, small)
        .map(|_| ())
        .map_err(|error| error.kind())
    {
        Err(JsonErrorKind::NodeLimit { limit }) if limit == nodes - 1 => Ok(()),
        other => Err(format!("node limit {}: {other:?}", nodes - 1)),
    }
}

fn check_rewrite(value: &Value) -> Result<(), String> {
    let mut canonical = String::new();
    value.write(&mut canonical);
    let source = targets::source(&canonical)?;
    let document = parse_json(&source, JsonDialect::Json, JsonLimits::default())
        .map_err(|error| format!("canonical rewrite rejected: {error}"))?;
    compare(&document, document.root(), value).map_err(|error| format!("rewrite: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_agrees_on_documented_cases() {
        for input in [
            "{\"a\": [1, -0.5e+3, true, null, \"x\\u00e9\\ud83d\\ude00\"], \"a\": {}}",
            "\u{feff} [ ] ",
            "[1,]",
            "// c\n{\"k\": 1, /* d */}",
            "\"\\ud800\"",
            "01",
            "[",
            "",
            "{\"a\" 1}",
            "\"\u{2028}\"",
        ] {
            run(input.as_bytes()).unwrap_or_else(|error| panic!("{input:?}: {error}"));
        }
        assert_eq!(Model::parse("[1,]", false, 256), None);
        assert_eq!(
            Model::parse("[1,]", true, 256),
            Some(Value::Array(vec![Value::Number("1".to_owned())]))
        );
    }
}
