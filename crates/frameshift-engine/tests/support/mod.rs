//! Test support: a temporary directory outside the repository and a strict
//! reader for the human-written `fixture.json` files.
//!
//! The reader accepts RFC 8259 JSON without fractions or exponents, rejects
//! duplicate keys, and keeps member order. It exists only for the fixture
//! harness until the planned product JSON parser of M2 replaces it.

#![allow(dead_code, reason = "each test crate uses a different part")]

use std::path::{Path, PathBuf};

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

/// Parses one complete JSON document.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = reader.value(0)?;
    reader.space();
    if reader.at != reader.bytes.len() {
        return Err(format!("trailing content at byte {}", reader.at));
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at byte {}", byte as char, self.at))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("invalid literal at byte {}", self.at))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > 32 {
            return Err("nesting too deep".to_owned());
        }
        self.space();
        match self.bytes.get(self.at) {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'0'..=b'9') => self.number(),
            _ => Err(format!("unexpected input at byte {}", self.at)),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut members: Vec<(String, Json)> = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.space();
            let key = self.string()?;
            if members.iter().any(|(name, _)| *name == key) {
                return Err(format!("duplicate key {key:?}"));
            }
            self.space();
            self.expect(b':')?;
            let value = self.value(depth + 1)?;
            members.push((key, value));
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.at)),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.at)),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        while matches!(self.bytes.get(self.at), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        let digits = std::str::from_utf8(&self.bytes[start..self.at]).unwrap();
        if digits.len() > 1 && digits.starts_with('0') {
            return Err(format!("leading zero at byte {start}"));
        }
        if matches!(self.bytes.get(self.at), Some(b'.' | b'e' | b'E')) {
            return Err(format!("only integers are supported, at byte {start}"));
        }
        digits
            .parse()
            .map(Json::Number)
            .map_err(|_| format!("number out of range at byte {start}"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or_else(|| "truncated escape".to_owned())?;
        let text = std::str::from_utf8(digits).map_err(|_| "invalid escape".to_owned())?;
        let value = u32::from_str_radix(text, 16).map_err(|_| "invalid escape".to_owned())?;
        self.at += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut text = String::new();
        loop {
            let start = self.at;
            while !matches!(self.bytes.get(self.at), None | Some(b'"' | b'\\')) {
                if self.bytes[self.at] < 0x20 {
                    return Err(format!("control character at byte {}", self.at));
                }
                self.at += 1;
            }
            text.push_str(std::str::from_utf8(&self.bytes[start..self.at]).unwrap());
            match self.bytes.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(text);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = *self
                        .bytes
                        .get(self.at)
                        .ok_or_else(|| "truncated escape".to_owned())?;
                    self.at += 1;
                    let decoded = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let high = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&high) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let low = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err("unpaired surrogate".to_owned());
                                }
                                0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                            } else {
                                high
                            };
                            char::from_u32(code).ok_or_else(|| "unpaired surrogate".to_owned())?
                        }
                        _ => return Err(format!("invalid escape at byte {}", self.at - 1)),
                    };
                    text.push(decoded);
                }
                _ => return Err("unterminated string".to_owned()),
            }
        }
    }
}
