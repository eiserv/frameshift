//! `.gitignore` parsing and matching with Git's pattern semantics (`D-065`).
//!
//! The behavior follows the gitignore(5) documentation and was checked
//! black-box against Git. A pattern compiles into tokens that are simulated
//! as a finite automaton, so matching takes time proportional to the product
//! of token count and subject length and never backtracks.

use std::fmt;

use frameshift_core::{Budget, BudgetExceeded};

/// Inclusive limits on reading and matching `.gitignore` files in one run.
///
/// Both are charged in canonical order by the discovery walker; exceeding
/// either stops discovery with a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IgnoreLimits {
    /// Maximum bytes of all `.gitignore` files read in one run. Defaults to 4 MiB.
    pub max_total_bytes: u64,
    /// Maximum matching work in one run, where every pattern matched against
    /// a subject charges (pattern bytes + 1) times (subject bytes + 1).
    /// Defaults to 2^34.
    pub max_match_work: u64,
}

impl Default for IgnoreLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 4 * 1024 * 1024,
            max_match_work: 1 << 34,
        }
    }
}

/// The effect of the last pattern in one file that matches a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoreDecision {
    /// A pattern without `!` matched, so the path is ignored.
    Ignored,
    /// A `!` pattern matched, so this file does not ignore the path.
    Included,
}

/// A `.gitignore` file that could not be stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoreError {
    /// Pattern storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for IgnoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed => f.write_str("ignore pattern storage allocation failed"),
        }
    }
}

impl std::error::Error for IgnoreError {}

/// The patterns of one `.gitignore` file, in file order.
///
/// Parsing never fails on content: every byte sequence is a valid file, and
/// malformed patterns, such as an unterminated bracket expression, match
/// nothing, as in Git.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitignoreFile {
    patterns: Vec<Pattern>,
}

impl GitignoreFile {
    /// Parses the exact bytes of a `.gitignore` file.
    ///
    /// A leading UTF-8 byte order mark is skipped. Lines end at LF, and one
    /// CR before the LF is removed. A line ends at its first NUL. Blank lines
    /// and lines that start with `#` are skipped, and trailing spaces are
    /// removed unless escaped with a backslash.
    pub fn parse(content: &[u8]) -> Result<Self, IgnoreError> {
        let content = content.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(content);
        let mut patterns = Vec::new();
        for line in content.split(|&byte| byte == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = match line.iter().position(|&byte| byte == 0) {
                Some(end) => &line[..end],
                None => line,
            };
            if line.first().is_none_or(|&byte| byte == b'#') {
                continue;
            }
            if let Some(pattern) = Pattern::parse(trim_trailing_spaces(line))? {
                patterns
                    .try_reserve(1)
                    .map_err(|_| IgnoreError::AllocationFailed)?;
                patterns.push(pattern);
            }
        }
        Ok(Self { patterns })
    }

    /// Returns the number of patterns that can match a path.
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// Returns whether no pattern can match a path.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Returns the effect of the last pattern that matches `path`, or `None`.
    ///
    /// `path` is relative to the directory of this file, with nonempty
    /// components separated by single slashes, and `is_directory` states
    /// whether it names a real directory (a link is not one). Patterns with a
    /// trailing slash apply only to directories. Patterns without an inner
    /// slash match the last component; the others match the whole path.
    /// Before each pattern is matched, `work` is charged (pattern bytes + 1)
    /// times (subject bytes + 1); a failed charge ends the decision with the
    /// error and leaves `work` unchanged.
    pub fn decide(
        &self,
        path: &str,
        is_directory: bool,
        work: &mut Budget,
    ) -> Result<Option<IgnoreDecision>, BudgetExceeded> {
        let name = path.rsplit('/').next().unwrap_or(path);
        for pattern in self.patterns.iter().rev() {
            if pattern.directory_only && !is_directory {
                continue;
            }
            let subject = if pattern.anchored { path } else { name };
            let cost = (pattern.weight + 1).saturating_mul(subject.len() as u64 + 1);
            work.charge(cost)?;
            if pattern.matches(subject.as_bytes()) {
                return Ok(Some(if pattern.negated {
                    IgnoreDecision::Included
                } else {
                    IgnoreDecision::Ignored
                }));
            }
        }
        Ok(None)
    }
}

/// Removes trailing spaces that are not escaped by a backslash.
fn trim_trailing_spaces(line: &[u8]) -> &[u8] {
    let mut first_trailing_space = None;
    let mut index = 0;
    while index < line.len() {
        match line[index] {
            b' ' => {
                first_trailing_space.get_or_insert(index);
            }
            b'\\' => {
                index += 1;
                if index == line.len() {
                    // A final backslash escapes nothing, and nothing is trimmed.
                    return line;
                }
                first_trailing_space = None;
            }
            _ => first_trailing_space = None,
        }
        index += 1;
    }
    match first_trailing_space {
        Some(end) => &line[..end],
        None => line,
    }
}

/// A set of bytes, never including `/`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ByteSet([u64; 4]);

impl ByteSet {
    fn insert(&mut self, byte: u8) {
        self.0[usize::from(byte >> 6)] |= 1 << (byte & 63);
    }

    fn insert_range(&mut self, first: u8, last: u8) {
        for byte in first..=last {
            self.insert(byte);
        }
    }

    fn contains(self, byte: u8) -> bool {
        self.0[usize::from(byte >> 6)] & (1 << (byte & 63)) != 0
    }

    fn complement(self) -> Self {
        Self(self.0.map(|word| !word))
    }

    fn without_slash(mut self) -> Self {
        self.0[0] &= !(1 << b'/');
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    /// Exactly this byte.
    Byte(u8),
    /// `?`: any byte except `/`.
    One,
    /// A bracket expression, by index into the pattern's sets.
    Set(u32),
    /// `*`, or `**` without slash delimiters: any bytes except `/`.
    Star,
    /// A delimited trailing `**`, or `**` before an escaped slash: any bytes.
    AnyPath,
    /// A delimited `**/`: nothing, or any bytes that end with `/`.
    Directories,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pattern {
    tokens: Vec<Token>,
    sets: Vec<ByteSet>,
    /// The pattern matches nothing, as Git does for a trailing backslash or a
    /// malformed bracket expression.
    never: bool,
    negated: bool,
    directory_only: bool,
    /// The pattern contains a slash and matches the whole relative path.
    anchored: bool,
    /// Pattern bytes after removing `!`, the trailing slash, and an anchored
    /// leading slash; the basis of the work charge.
    weight: u64,
}

impl Pattern {
    /// Parses one trimmed line, or returns `None` for a pattern that matches
    /// nothing because it is empty.
    fn parse(line: &[u8]) -> Result<Option<Self>, IgnoreError> {
        let (negated, rest) = match line.strip_prefix(b"!") {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let (directory_only, body) = match rest.strip_suffix(b"/") {
            Some(body) => (true, body),
            None => (false, rest),
        };
        let anchored = body.contains(&b'/');
        let body = if anchored {
            body.strip_prefix(b"/").unwrap_or(body)
        } else {
            body
        };
        if body.is_empty() {
            return Ok(None);
        }
        let mut pattern = Self {
            tokens: Vec::new(),
            sets: Vec::new(),
            never: false,
            negated,
            directory_only,
            anchored,
            weight: body.len() as u64,
        };
        pattern.compile(body)?;
        Ok(Some(pattern))
    }

    fn compile(&mut self, body: &[u8]) -> Result<(), IgnoreError> {
        self.tokens
            .try_reserve_exact(body.len())
            .map_err(|_| IgnoreError::AllocationFailed)?;
        let mut index = 0;
        while index < body.len() {
            let token = match body[index] {
                b'\\' => match body.get(index + 1) {
                    Some(&byte) => {
                        index += 2;
                        Token::Byte(byte)
                    }
                    None => {
                        self.never = true;
                        return Ok(());
                    }
                },
                b'?' => {
                    index += 1;
                    Token::One
                }
                b'*' => {
                    let start = index;
                    while body.get(index) == Some(&b'*') {
                        index += 1;
                    }
                    self.star(body, start, &mut index)
                }
                b'[' => match bracket(body, index) {
                    Some((set, next)) => {
                        index = next;
                        self.sets
                            .try_reserve(1)
                            .map_err(|_| IgnoreError::AllocationFailed)?;
                        self.sets.push(set);
                        Token::Set((self.sets.len() - 1) as u32)
                    }
                    None => {
                        self.never = true;
                        return Ok(());
                    }
                },
                byte => {
                    index += 1;
                    Token::Byte(byte)
                }
            };
            self.tokens.push(token);
        }
        Ok(())
    }

    /// Classifies a run of asterisks from `start` to `*index`. Only an
    /// anchored pattern gives `**` its special meanings, and only when it
    /// starts the pattern or follows `/`, and ends the pattern or precedes
    /// `/` or an escaped `/`.
    fn star(&self, body: &[u8], start: usize, index: &mut usize) -> Token {
        let delimited_before = start == 0 || body[start - 1] == b'/';
        if !self.anchored || *index - start < 2 || !delimited_before {
            return Token::Star;
        }
        let after = &body[*index..];
        if after.is_empty() || after.starts_with(b"\\/") {
            Token::AnyPath
        } else if after[0] == b'/' {
            *index += 1;
            Token::Directories
        } else {
            Token::Star
        }
    }

    fn matches(&self, subject: &[u8]) -> bool {
        if self.never {
            return false;
        }
        let words = self.tokens.len() / 64 + 1;
        if words <= 4 {
            let mut buffers = [[0_u64; 4]; 4];
            let [current, next, loops, next_loops] = &mut buffers;
            self.simulate(
                subject,
                &mut current[..words],
                &mut next[..words],
                &mut loops[..words],
                &mut next_loops[..words],
            )
        } else {
            let mut current = vec![0; words];
            let mut next = vec![0; words];
            let mut loops = vec![0; words];
            let mut next_loops = vec![0; words];
            self.simulate(
                subject,
                &mut current,
                &mut next,
                &mut loops,
                &mut next_loops,
            )
        }
    }

    /// Runs the automaton. State `i` means that tokens before `i` matched;
    /// a loop state of a `Directories` token means that it consumed bytes
    /// and waits for the `/` that ends them.
    fn simulate<'s>(
        &self,
        subject: &[u8],
        mut current: &'s mut [u64],
        mut next: &'s mut [u64],
        mut loops: &'s mut [u64],
        mut next_loops: &'s mut [u64],
    ) -> bool {
        let end = self.tokens.len();
        set(current, 0);
        self.close(current, loops);
        for &byte in subject {
            next.fill(0);
            next_loops.fill(0);
            let mut alive = false;
            for (state, &token) in self.tokens.iter().enumerate() {
                if get(loops, state) {
                    set(next_loops, state);
                    if byte == b'/' {
                        set(next, state + 1);
                    }
                    alive = true;
                }
                if !get(current, state) {
                    continue;
                }
                alive = true;
                let target = match token {
                    Token::Byte(expected) => (byte == expected).then_some(state + 1),
                    Token::One => (byte != b'/').then_some(state + 1),
                    Token::Set(set_index) => self.sets[set_index as usize]
                        .contains(byte)
                        .then_some(state + 1),
                    Token::Star => (byte != b'/').then_some(state),
                    Token::AnyPath => Some(state),
                    Token::Directories => None,
                };
                if let Some(target) = target {
                    set(next, target);
                }
            }
            if !alive {
                return false;
            }
            self.close(next, next_loops);
            std::mem::swap(&mut current, &mut next);
            std::mem::swap(&mut loops, &mut next_loops);
        }
        get(current, end)
    }

    /// Adds the states reachable without consuming a byte. Such transitions
    /// only advance, so one ascending pass suffices.
    fn close(&self, states: &mut [u64], loops: &mut [u64]) {
        for (state, &token) in self.tokens.iter().enumerate() {
            if !get(states, state) {
                continue;
            }
            match token {
                Token::Star | Token::AnyPath => set(states, state + 1),
                Token::Directories => {
                    set(states, state + 1);
                    set(loops, state);
                }
                Token::Byte(_) | Token::One | Token::Set(_) => {}
            }
        }
    }
}

fn get(bits: &[u64], index: usize) -> bool {
    bits[index / 64] & (1 << (index % 64)) != 0
}

fn set(bits: &mut [u64], index: usize) {
    bits[index / 64] |= 1 << (index % 64);
}

/// Parses the bracket expression that starts at `open`, returning its byte
/// set without `/` and the index after the closing `]`, or `None` when it
/// is unterminated or names an unknown character class.
fn bracket(body: &[u8], open: usize) -> Option<(ByteSet, usize)> {
    let mut index = open + 1;
    let negated = matches!(body.get(index), Some(b'!' | b'^'));
    if negated {
        index += 1;
    }
    let mut set = ByteSet::default();
    // The previous member, when it was a single byte that may start a range.
    let mut previous: Option<u8> = None;
    let mut first = true;
    loop {
        let &byte = body.get(index)?;
        if byte == b']' && !first {
            index += 1;
            break;
        }
        first = false;
        if byte == b'\\' {
            let &literal = body.get(index + 1)?;
            set.insert(literal);
            previous = Some(literal);
            index += 2;
        } else if let (b'-', Some(low), Some(&high)) = (byte, previous, body.get(index + 1))
            && high != b']'
        {
            let (high, width) = if high == b'\\' {
                (*body.get(index + 2)?, 3)
            } else {
                (high, 2)
            };
            if low <= high {
                set.insert_range(low, high);
            }
            previous = None;
            index += width;
        } else if byte == b'[' && body.get(index + 1) == Some(&b':') {
            let name_start = index + 2;
            let close = name_start + body[name_start..].iter().position(|&b| b == b']')?;
            if close > name_start && body[close - 1] == b':' {
                class(&mut set, &body[name_start..close - 1])?;
                previous = None;
                index = close + 1;
            } else {
                // Without `:]`, the `[` is an ordinary member.
                set.insert(b'[');
                previous = Some(b'[');
                index += 1;
            }
        } else {
            set.insert(byte);
            previous = Some(byte);
            index += 1;
        }
    }
    let set = if negated { set.complement() } else { set };
    Some((set.without_slash(), index))
}

/// Adds the ASCII members of a POSIX character class, as Git's own
/// locale-independent character table defines them.
fn class(set: &mut ByteSet, name: &[u8]) -> Option<()> {
    let ranges: &[(u8, u8)] = match name {
        b"alnum" => &[(b'0', b'9'), (b'A', b'Z'), (b'a', b'z')],
        b"alpha" => &[(b'A', b'Z'), (b'a', b'z')],
        b"blank" => &[(b'\t', b'\t'), (b' ', b' ')],
        b"cntrl" => &[(0x00, 0x1F), (0x7F, 0x7F)],
        b"digit" => &[(b'0', b'9')],
        b"graph" => &[(0x21, 0x7E)],
        b"lower" => &[(b'a', b'z')],
        b"print" => &[(0x20, 0x7E)],
        b"punct" => &[(b'!', b'/'), (b':', b'@'), (b'[', b'`'), (b'{', b'~')],
        b"space" => &[(b'\t', b'\n'), (b'\r', b'\r'), (b' ', b' ')],
        b"upper" => &[(b'A', b'Z')],
        b"xdigit" => &[(b'0', b'9'), (b'A', b'F'), (b'a', b'f')],
        _ => return None,
    };
    for &(low, high) in ranges {
        set.insert_range(low, high);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(content: &[u8], path: &str, is_directory: bool) -> Option<IgnoreDecision> {
        let file = GitignoreFile::parse(content).unwrap();
        file.decide(path, is_directory, &mut Budget::new(u64::MAX))
            .unwrap()
    }

    #[test]
    fn trailing_spaces_follow_escapes() {
        assert_eq!(trim_trailing_spaces(b"a  "), b"a");
        assert_eq!(trim_trailing_spaces(b"a\\  "), b"a\\ ");
        assert_eq!(trim_trailing_spaces(b"a \\"), b"a \\");
        assert_eq!(trim_trailing_spaces(b"   "), b"");
        assert_eq!(trim_trailing_spaces(b"a\t "), b"a\t");
    }

    #[test]
    fn large_patterns_use_heap_states() {
        let long = format!("{}*", "a".repeat(300));
        let content = long.as_bytes();
        assert_eq!(
            decide(content, &"a".repeat(305), false),
            Some(IgnoreDecision::Ignored)
        );
        assert_eq!(decide(content, &"a".repeat(299), false), None);
    }

    #[test]
    fn work_is_charged_before_matching_and_failures_change_nothing() {
        let file = GitignoreFile::parse(b"abc\nx/y\n").unwrap();
        // `x/y` is anchored: (3 + 1) * (5 + 1); `abc` matches the name: (3 + 1) * (1 + 1).
        let mut work = Budget::new(24 + 8);
        assert_eq!(file.decide("d/e/f", false, &mut work), Ok(None));
        assert_eq!(work.used(), 32);
        let mut work = Budget::new(31);
        assert!(file.decide("d/e/f", false, &mut work).is_err());
        assert_eq!(work.used(), 24);
        let mut directories = Budget::new(0);
        let only = GitignoreFile::parse(b"dir/\n").unwrap();
        assert_eq!(only.decide("dir", false, &mut directories), Ok(None));
    }
}
