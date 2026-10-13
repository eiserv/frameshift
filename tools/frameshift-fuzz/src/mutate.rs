//! Deterministic input mutation (`D-044`).
//!
//! Each new input applies one to eight random operations to a corpus input:
//! bit flips and byte replacements, insertions and deletions of syntax
//! bytes, duplicated ranges, tokens from the target's dictionary, repeated
//! tokens that build deep nesting, scalars that the lexical grammar treats
//! specially, and ranges spliced from a second corpus input. Apart from the
//! dictionary, the operations know no grammar, so inputs also reach
//! malformed text and bytes that are not UTF-8. The seed determines every
//! choice.

use frameshift_corpus::Rng;

/// Bytes that start, end, or separate JavaScript, JSX, and TypeScript tokens.
const SYNTAX_BYTES: &[u8] = b"{}[]()<>/\\'\"`$#@!?:;,.=+-*%&|^~ \t\r\n0179aenuxAZ_";

/// Scalars beyond ASCII with roles in the lexical grammar: whitespace, line
/// terminators, the byte order mark, joiners, identifier characters, and
/// astral characters that need surrogate pairs.
const SCALARS: [char; 12] = [
    '\u{00A0}',
    '\u{2028}',
    '\u{2029}',
    '\u{FEFF}',
    '\u{200C}',
    '\u{200D}',
    '\u{0085}',
    '\u{3000}',
    '\u{00E9}',
    '\u{2135}',
    '\u{1D49C}',
    '\u{1F600}',
];

/// The most copies of one token that a single operation inserts, enough to
/// pass the parser's default nesting bound of 256.
const MAX_REPETITIONS: u64 = 300;

/// The longest range that one operation deletes, duplicates, or splices.
const MAX_RANGE: usize = 64;

/// Derives new inputs from corpus inputs with a seeded random sequence.
pub struct Mutator {
    rng: Rng,
    max_len: usize,
}

impl Mutator {
    /// Creates a mutator whose inputs never exceed `max_len` bytes.
    pub fn new(seed: u64, max_len: usize) -> Self {
        Self {
            rng: Rng::new(seed),
            max_len,
        }
    }

    /// Returns a value in `0..bound`; `bound` must be positive.
    pub fn below(&mut self, bound: usize) -> usize {
        self.rng.below(bound as u64) as usize
    }

    /// Replaces `out` with a mutation of `base`, which may splice ranges from
    /// `other` and insert tokens from `dictionary`.
    pub fn mutate(&mut self, base: &[u8], other: &[u8], dictionary: &[&str], out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(base);
        let operations = 1 + self.below(8);
        for _ in 0..operations {
            self.apply(out, other, dictionary);
            // Bounding each step keeps repeated insertions from growing the
            // buffer far beyond the limit before the final truncation.
            out.truncate(self.max_len.saturating_mul(2));
        }
        out.truncate(self.max_len);
    }

    fn apply(&mut self, out: &mut Vec<u8>, other: &[u8], dictionary: &[&str]) {
        match self.below(12) {
            0 if !out.is_empty() => {
                let index = self.below(out.len());
                out[index] ^= 1 << self.below(8);
            }
            1 if !out.is_empty() => {
                let index = self.below(out.len());
                out[index] = self.below(256) as u8;
            }
            2 if !out.is_empty() => {
                let index = self.below(out.len());
                out[index] = self.syntax_byte();
            }
            3 => {
                let position = self.below(out.len() + 1);
                let count = 1 + self.below(4);
                let bytes: Vec<u8> = (0..count).map(|_| self.syntax_byte()).collect();
                out.splice(position..position, bytes);
            }
            4 if !out.is_empty() => {
                let (start, end) = self.range(out.len());
                out.drain(start..end);
            }
            5 if !out.is_empty() => {
                let (start, end) = self.range(out.len());
                let copy = out[start..end].to_vec();
                let position = self.below(out.len() + 1);
                out.splice(position..position, copy);
            }
            6 if !dictionary.is_empty() => {
                let token = dictionary[self.below(dictionary.len())].as_bytes();
                let position = self.below(out.len() + 1);
                out.splice(position..position, token.iter().copied());
            }
            7 if !dictionary.is_empty() && !out.is_empty() => {
                let token = dictionary[self.below(dictionary.len())].as_bytes();
                let (start, end) = self.range(out.len());
                out.splice(start..end, token.iter().copied());
            }
            8 if !dictionary.is_empty() => {
                let token = dictionary[self.below(dictionary.len())].as_bytes();
                let copies = 2 + self.rng.below(MAX_REPETITIONS - 1) as usize;
                let position = self.below(out.len() + 1);
                let repeated = token.repeat(copies.min(self.max_len / token.len().max(1) + 1));
                out.splice(position..position, repeated);
            }
            9 => {
                let scalar = SCALARS[self.below(SCALARS.len())];
                let mut encoded = [0; 4];
                let position = self.below(out.len() + 1);
                out.splice(position..position, scalar.encode_utf8(&mut encoded).bytes());
            }
            10 if !other.is_empty() => {
                let (start, end) = self.range(other.len());
                let position = self.below(out.len() + 1);
                out.splice(position..position, other[start..end].iter().copied());
            }
            11 if !other.is_empty() => {
                out.truncate(self.below(out.len() + 1));
                let from = self.below(other.len());
                out.extend_from_slice(&other[from..]);
            }
            // An operation that does not apply to an empty input or an
            // empty dictionary inserts a syntax byte instead, so every step
            // changes the input.
            _ => {
                let position = self.below(out.len() + 1);
                let byte = self.syntax_byte();
                out.insert(position, byte);
            }
        }
    }

    fn syntax_byte(&mut self) -> u8 {
        SYNTAX_BYTES[self.below(SYNTAX_BYTES.len())]
    }

    // A nonempty range of at most MAX_RANGE bytes inside `0..len`, which
    // must be positive.
    fn range(&mut self, len: usize) -> (usize, usize) {
        let start = self.below(len);
        let end = start + 1 + self.below((len - start).min(MAX_RANGE));
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DICTIONARY: [&str; 3] = ["function", "(", "=>"];

    fn run(seed: u64, base: &[u8], other: &[u8], rounds: usize) -> Vec<Vec<u8>> {
        let mut mutator = Mutator::new(seed, 64);
        let mut out = Vec::new();
        (0..rounds)
            .map(|_| {
                mutator.mutate(base, other, &DICTIONARY, &mut out);
                out.clone()
            })
            .collect()
    }

    #[test]
    fn mutation_is_deterministic_and_bounded() {
        let base = b"const value = call(a, b);";
        let other = b"class Box { #x = 1; }";
        let first = run(7, base, other, 2_000);
        assert_eq!(first, run(7, base, other, 2_000));
        assert_ne!(first, run(8, base, other, 2_000));
        assert!(first.iter().all(|input| input.len() <= 64));
        assert!(first.iter().any(|input| input.as_slice() != base));
        // Dictionary tokens, splices, and repetitions all occur.
        assert!(
            first
                .iter()
                .any(|input| input.windows(8).any(|w| w == b"function"))
        );
        assert!(
            first
                .iter()
                .any(|input| input.windows(5).any(|w| w == b"class"))
        );
        assert!(
            first
                .iter()
                .any(|input| input.windows(4).any(|w| w == b"(((("))
        );
        // Some inputs are no longer UTF-8, so text validation is exercised.
        assert!(
            first
                .iter()
                .any(|input| std::str::from_utf8(input).is_err())
        );
    }

    #[test]
    fn empty_inputs_and_dictionaries_still_mutate() {
        let mut mutator = Mutator::new(1, 16);
        let mut out = Vec::new();
        let mut nonempty = 0;
        for _ in 0..1_000 {
            mutator.mutate(b"", b"", &[], &mut out);
            assert!(out.len() <= 16);
            nonempty += usize::from(!out.is_empty());
        }
        // A later deletion can remove what an earlier step inserted.
        assert!(nonempty > 900, "{nonempty}");
    }
}
