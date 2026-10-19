//! Test support: a temporary directory outside the repository and a strict
//! reader for the human-written `fixture.json` files.
//!
//! The reader uses the product JSON parser, keeps member order, and rejects
//! duplicate keys and numbers other than nonnegative integers.

#![allow(dead_code, reason = "each test crate uses a different part")]

use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{
    JsonDialect, JsonDocument, JsonId, JsonLimits, JsonValue, parse_json,
};

/// Cargo's temporary directory, or the system one when a test binary runs on
/// another host than it was built for, such as a Linux binary built on
/// Windows and run in WSL, where the embedded Windows path is not absolute.
pub fn temporary() -> PathBuf {
    let cargo = Path::new(env!("CARGO_TARGET_TMPDIR"));
    if cargo.is_absolute() {
        cargo.to_path_buf()
    } else {
        std::env::temp_dir().join("frameshift-tests")
    }
}

/// A JSON value with nonnegative integer numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Returns the member of an object, if present.
    pub fn get(&self, key: &str) -> Option<&Json> {
        let Self::Object(members) = self else {
            return None;
        };
        members
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// Returns the member names of an object.
    pub fn keys(&self) -> Vec<&str> {
        let Self::Object(members) = self else {
            return Vec::new();
        };
        members.iter().map(|(name, _)| name.as_str()).collect()
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(text) = self {
            Some(text)
        } else {
            None
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        if let Self::Number(number) = self {
            Some(*number)
        } else {
            None
        }
    }
}

/// Parses one complete `fixture.json` document with the product JSON
/// parser (`D-068`), more strictly than products do: duplicate keys and
/// numbers other than nonnegative integers are rejected.
pub fn parse(text: &str) -> Result<Json, String> {
    let source = SourceText::new(text.to_owned(), TextLimits::default())
        .map_err(|error| error.to_string())?;
    let document = parse_json(&source, JsonDialect::Json, JsonLimits::default())
        .map_err(|error| error.to_string())?;
    if let Some(member) = document.duplicate_keys().next() {
        return Err(format!("duplicate key {:?}", member.key()));
    }
    convert(&document, document.root())
}

fn convert(document: &JsonDocument<'_>, id: JsonId) -> Result<Json, String> {
    Ok(match document.value(id).ok_or("unknown value")? {
        JsonValue::Null => Json::Null,
        JsonValue::Bool(value) => Json::Bool(value),
        JsonValue::Number(number) => Json::Number(number.as_u64().ok_or_else(|| {
            format!("only nonnegative integers are supported: {}", number.text())
        })?),
        JsonValue::String(text) => Json::String(text.to_owned()),
        JsonValue::Array(array) => Json::Array(
            array
                .iter()
                .map(|item| convert(document, item))
                .collect::<Result<_, _>>()?,
        ),
        JsonValue::Object(object) => Json::Object(
            object
                .members()
                .map(|member| Ok((member.key().to_owned(), convert(document, member.value())?)))
                .collect::<Result<_, String>>()?,
        ),
    })
}
