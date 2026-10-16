//! `.gitignore` parsing and matching against an independent model (`D-065`).
//!
//! The input holds a `.gitignore` file, a double NUL, and newline-separated
//! paths whose first byte selects a directory (odd) or another entry (even).
//! The model interprets pattern bytes directly by memoized recursion, a
//! different technique from the product's automaton, and computes the
//! documented work charge independently.

use frameshift_engine::foundation::Budget;
use frameshift_engine::input::{GitignoreFile, IgnoreDecision};

/// Checks decisions, work charges, atomic budget failures, and repetition.
pub fn run(input: &[u8]) -> Result<(), String> {
    let split = input.windows(2).position(|pair| pair == [0, 0]);
    let (content, rest) = match split {
        Some(at) => (&input[..at], &input[at + 2..]),
        None => (input, &[][..]),
    };
    let file = GitignoreFile::parse(content).map_err(|error| error.to_string())?;
    if GitignoreFile::parse(content).map_err(|error| error.to_string())? != file {
        return Err("parsing is not repeatable".to_owned());
    }
    let model = model_parse(content);
    if model.len() != file.len() {
        return Err(format!("{} patterns, model {}", file.len(), model.len()));
    }
    for line in rest.split(|&byte| byte == b'\n') {
        let Some((&selector, path)) = line.split_first() else {
            continue;
        };
        let Ok(path) = std::str::from_utf8(path) else {
            continue;
        };
        if path.is_empty() || path.split('/').any(str::is_empty) {
            continue;
        }
        let directory = selector % 2 == 1;
        let (expected, charge) = model_decide(&model, path, directory);
        let mut work = Budget::new(u64::MAX);
        let actual = file
            .decide(path, directory, &mut work)
            .map_err(|_| "unbounded budget failed".to_owned())?;
        if actual != expected {
            return Err(format!(
                "{path:?} directory={directory}: {actual:?}, model {expected:?}"
            ));
        }
        if work.used() != charge {
            return Err(format!("{path:?}: charged {}, model {charge}", work.used()));
        }
        let mut again = Budget::new(u64::MAX);
        if file.decide(path, directory, &mut again) != Ok(actual) || again.used() != charge {
            return Err(format!("{path:?}: decision is not repeatable"));
        }
        if charge > 0 {
            let mut short = Budget::new(charge - 1);
            match file.decide(path, directory, &mut short) {
                Err(error) if error.limit() == charge - 1 && short.used() < charge => {}
                other => {
                    return Err(format!("{path:?}: short budget gave {other:?}"));
                }
            }
        }
    }
    Ok(())
}

struct ModelPattern {
    body: Vec<u8>,
    negated: bool,
    directory_only: bool,
    anchored: bool,
}

/// Splits lines as Git does and keeps the patterns that can match.
fn model_parse(content: &[u8]) -> Vec<ModelPattern> {
    let content = match content {
        [0xEF, 0xBB, 0xBF, rest @ ..] => rest,
        other => other,
    };
    let mut patterns = Vec::new();
    for raw in content.split(|&byte| byte == b'\n') {
        let mut line: Vec<u8> = raw.to_vec();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if let Some(nul) = line.iter().position(|&byte| byte == 0) {
            line.truncate(nul);
        }
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        // Keep everything up to the last byte that is not an unescaped
        // space; a final backslash that escapes nothing disables trimming.
        let mut keep = 0;
        let mut index = 0;
        while index < line.len() {
            if line[index] == b'\\' {
                if index + 1 == line.len() {
                    keep = line.len();
                    break;
                }
                index += 2;
                keep = index;
            } else {
                if line[index] != b' ' {
                    keep = index + 1;
                }
                index += 1;
            }
        }
        line.truncate(keep);
        let negated = line.first() == Some(&b'!');
        if negated {
            line.remove(0);
        }
        let directory_only = line.last() == Some(&b'/');
        if directory_only {
            line.pop();
        }
        let anchored = line.contains(&b'/');
        if anchored && line.first() == Some(&b'/') {
            line.remove(0);
        }
        if !line.is_empty() {
            patterns.push(ModelPattern {
                body: line,
                negated,
                directory_only,
                anchored,
            });
        }
    }
    patterns
}

fn model_decide(
    patterns: &[ModelPattern],
    path: &str,
    directory: bool,
) -> (Option<IgnoreDecision>, u64) {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut charge = 0_u64;
    for pattern in patterns.iter().rev() {
        if pattern.directory_only && !directory {
            continue;
        }
        let subject = if pattern.anchored { path } else { name };
        charge += (pattern.body.len() as u64 + 1) * (subject.len() as u64 + 1);
        if Matcher::new(&pattern.body, subject.as_bytes(), pattern.anchored).matches() {
            let decision = if pattern.negated {
                IgnoreDecision::Included
            } else {
                IgnoreDecision::Ignored
            };
            return (Some(decision), charge);
        }
    }
    (None, charge)
}

/// Memoized recursive interpretation of raw pattern bytes.
struct Matcher<'a> {
    pattern: &'a [u8],
    subject: &'a [u8],
    pathname: bool,
    memo: Vec<Option<bool>>,
}

impl<'a> Matcher<'a> {
    fn new(pattern: &'a [u8], subject: &'a [u8], pathname: bool) -> Self {
        Self {
            pattern,
            subject,
            pathname,
            memo: vec![None; (pattern.len() + 1) * (subject.len() + 1)],
        }
    }

    fn matches(&mut self) -> bool {
        self.at(0, 0)
    }

    fn at(&mut self, p: usize, t: usize) -> bool {
        let slot = p * (self.subject.len() + 1) + t;
        if let Some(known) = self.memo[slot] {
            return known;
        }
        let result = self.compute(p, t);
        self.memo[slot] = Some(result);
        result
    }

    fn byte(&self, t: usize) -> Option<u8> {
        self.subject.get(t).copied()
    }

    fn compute(&mut self, p: usize, t: usize) -> bool {
        let Some(&current) = self.pattern.get(p) else {
            return t == self.subject.len();
        };
        match current {
            b'\\' => match self.pattern.get(p + 1) {
                Some(&literal) => self.byte(t) == Some(literal) && self.at(p + 2, t + 1),
                None => false,
            },
            b'?' => self.byte(t).is_some_and(|byte| byte != b'/') && self.at(p + 1, t + 1),
            b'[' => match model_bracket(self.pattern, p) {
                None => false,
                Some((members, next)) => {
                    self.byte(t)
                        .is_some_and(|byte| byte != b'/' && members[usize::from(byte)])
                        && self.at(next, t + 1)
                }
            },
            b'*' => {
                let mut q = p;
                while self.pattern.get(q) == Some(&b'*') {
                    q += 1;
                }
                let after = &self.pattern[q..];
                let delimited = self.pathname
                    && q - p >= 2
                    && (p == 0 || self.pattern[p - 1] == b'/')
                    && (after.is_empty() || after[0] == b'/' || after.starts_with(b"\\/"));
                if delimited && after.is_empty() {
                    return true;
                }
                if delimited && after[0] == b'/' {
                    // Zero directories, or any bytes up to and including a slash.
                    return self.at(q + 1, t)
                        || (t..self.subject.len())
                            .any(|k| self.subject[k] == b'/' && self.at(q + 1, k + 1));
                }
                if delimited {
                    return (t..self.subject.len())
                        .any(|k| self.subject[k] == b'/' && self.at(q + 2, k + 1));
                }
                let mut k = t;
                loop {
                    if self.at(q, k) {
                        return true;
                    }
                    match self.byte(k) {
                        Some(byte) if byte != b'/' => k += 1,
                        _ => return false,
                    }
                }
            }
            literal => self.byte(t) == Some(literal) && self.at(p + 1, t + 1),
        }
    }
}

/// Bracket expression members, or `None` when it is unterminated or names
/// an unknown class, which makes the whole pattern match nothing.
fn model_bracket(pattern: &[u8], open: usize) -> Option<([bool; 256], usize)> {
    let mut members = [false; 256];
    let mut i = open + 1;
    let negated = matches!(pattern.get(i), Some(b'!' | b'^'));
    if negated {
        i += 1;
    }
    let mut range_start: Option<u8> = None;
    let mut position = 0;
    loop {
        let byte = *pattern.get(i)?;
        if byte == b']' && position > 0 {
            i += 1;
            break;
        }
        position += 1;
        if byte == b'\\' {
            let literal = *pattern.get(i + 1)?;
            members[usize::from(literal)] = true;
            range_start = Some(literal);
            i += 2;
            continue;
        }
        if byte == b'-'
            && let Some(start) = range_start
            && let Some(&end) = pattern.get(i + 1)
            && end != b']'
        {
            let (end, width) = if end == b'\\' {
                (*pattern.get(i + 2)?, 3)
            } else {
                (end, 2)
            };
            for value in start..=end {
                members[usize::from(value)] = true;
            }
            range_start = None;
            i += width;
            continue;
        }
        if byte == b'[' && pattern.get(i + 1) == Some(&b':') {
            let close = i + 2 + pattern[i + 2..].iter().position(|&b| b == b']')?;
            if close >= i + 3 && pattern[close - 1] == b':' {
                let name = &pattern[i + 2..close - 1];
                let test: fn(u8) -> bool = match name {
                    b"alnum" => |b| b.is_ascii_alphanumeric(),
                    b"alpha" => |b| b.is_ascii_alphabetic(),
                    b"blank" => |b| b == b' ' || b == b'\t',
                    b"cntrl" => |b| b.is_ascii_control(),
                    b"digit" => |b| b.is_ascii_digit(),
                    b"graph" => |b| b.is_ascii_graphic(),
                    b"lower" => |b| b.is_ascii_lowercase(),
                    b"print" => |b| b.is_ascii_graphic() || b == b' ',
                    b"punct" => |b| b.is_ascii_punctuation(),
                    // Git's own table: no vertical tab or form feed.
                    b"space" => |b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'),
                    b"upper" => |b| b.is_ascii_uppercase(),
                    b"xdigit" => |b| b.is_ascii_hexdigit(),
                    _ => return None,
                };
                for value in 0..=255_u8 {
                    if test(value) {
                        members[usize::from(value)] = true;
                    }
                }
                range_start = None;
                i = close + 1;
                continue;
            }
        }
        members[usize::from(byte)] = true;
        range_start = Some(byte);
        i += 1;
    }
    if negated {
        for member in &mut members {
            *member = !*member;
        }
    }
    Some((members, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_agrees_on_documented_examples() {
        let cases: [(&[u8], &str, bool, Option<IgnoreDecision>); 8] = [
            (b"**/foo\n", "a/b/foo", false, Some(IgnoreDecision::Ignored)),
            (b"a/**/b\n", "a/b", false, Some(IgnoreDecision::Ignored)),
            (b"**\\/x\n", "x", false, None),
            (b"x/*\n", "x/a/b", false, None),
            (b"[[:space:]]\n", "\u{b}", false, None),
            (
                b"*.log\n!keep.log\n",
                "keep.log",
                false,
                Some(IgnoreDecision::Included),
            ),
            (b"dir/\n", "dir", false, None),
            (b"a\\  \n", "a ", false, Some(IgnoreDecision::Ignored)),
        ];
        for (content, path, directory, expected) in cases {
            let model = model_parse(content);
            assert_eq!(model_decide(&model, path, directory).0, expected, "{path}");
            let mut input = content.to_vec();
            input.extend_from_slice(b"\0\0");
            input.push(u8::from(directory));
            input.extend_from_slice(path.as_bytes());
            run(&input).unwrap();
        }
    }
}
