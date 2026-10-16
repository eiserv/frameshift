//! Fixed SHA-256 identities with bounded, allocation-free streaming.

use std::{fmt, str::FromStr};

use sha2::{Digest, Sha256};

const MAX_BYTES: u64 = u64::MAX / 8;

/// Explicit inclusive bounds on the bytes hashed by one operation.
///
/// The SHA-256 algorithm permits fewer than 2^64 input bits. A larger configured
/// limit is capped at that algorithm boundary. Callers set smaller limits for
/// their subsystem; this value does not limit file reads or whole-run memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashLimits {
    /// Maximum cumulative input bytes. Defaults to 2^61 - 1.
    pub max_bytes: u64,
}

impl Default for HashLimits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_BYTES,
        }
    }
}

impl HashLimits {
    /// Returns the enforced bound: the lesser of `max_bytes` and 2^61 - 1.
    pub const fn effective_max_bytes(self) -> u64 {
        if self.max_bytes < MAX_BYTES {
            self.max_bytes
        } else {
            MAX_BYTES
        }
    }
}

/// A hashing or canonical digest parsing failure, without input contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashError {
    /// The next complete update would exceed the effective input limit.
    InputTooLong {
        /// The lesser of the configured and SHA-256 algorithm byte limits.
        limit: u64,
    },
    /// A serialized digest does not contain exactly 71 UTF-8 bytes.
    InvalidLength,
    /// A serialized digest does not begin with exactly `sha256:`.
    InvalidPrefix,
    /// A digest contains a byte other than a lowercase hexadecimal digit.
    InvalidHex {
        /// Byte offset in the complete serialized digest.
        offset: usize,
    },
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { limit } => write!(f, "hash input exceeds byte limit {limit}"),
            Self::InvalidLength => f.write_str("SHA-256 digest must contain exactly 71 bytes"),
            Self::InvalidPrefix => f.write_str("SHA-256 digest must begin with sha256:"),
            Self::InvalidHex { offset } => {
                write!(
                    f,
                    "SHA-256 digest has invalid lowercase hex at byte {offset}"
                )
            }
        }
    }
}

impl std::error::Error for HashError {}

/// A SHA-256 digest, ordered by its 32 bytes and displayed as `sha256:<64 hex>`.
///
/// Hexadecimal digits are lowercase. This is a content identity, not a
/// signature, authentication token, or proof of behavioral equivalence.
/// Construction and parsing use fixed storage. Formatting writes directly to
/// the caller's sink, which may allocate its own output storage.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// Constructs a digest from its 32 raw bytes, without hashing those bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrows the raw digest bytes in SHA-256 output order.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hashes every input byte exactly, including BOMs and line endings.
    ///
    /// No normalization, text decoding, or domain framing is performed.
    pub fn hash(input: &[u8], limits: HashLimits) -> Result<Self, HashError> {
        let mut hasher = Sha256Hasher::new(limits);
        hasher.update(input)?;
        Ok(hasher.finish())
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sha256:")?;
        fmt::LowerHex::fmt(self, f)
    }
}

impl fmt::LowerHex for Sha256Digest {
    /// Writes exactly 64 lowercase hexadecimal digits, without an algorithm prefix.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Sha256Digest {
    type Err = HashError;

    /// Parses exactly `sha256:` followed by 64 lowercase hexadecimal digits.
    ///
    /// Length is checked before the prefix, then digits in byte order. Input
    /// is never trimmed, case-folded, or echoed in errors.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if bytes.len() != 71 {
            return Err(HashError::InvalidLength);
        }
        if !bytes.starts_with(b"sha256:") {
            return Err(HashError::InvalidPrefix);
        }
        let mut result = [0; 32];
        for (index, pair) in bytes[7..].as_chunks::<2>().0.iter().enumerate() {
            let high = hex_digit(pair[0], 7 + 2 * index)?;
            let low = hex_digit(pair[1], 8 + 2 * index)?;
            result[index] = (high << 4) | low;
        }
        Ok(Self(result))
    }
}

fn hex_digit(byte: u8, offset: usize) -> Result<u8, HashError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(HashError::InvalidHex { offset }),
    }
}

/// An allocation-free SHA-256 stream with explicit cumulative byte limits.
///
/// Updates concatenate raw bytes; chunk boundaries have no identity. Future
/// snapshot and plan formats must define their own unambiguous framing and
/// domain separation. This API exposes no third-party types or mutable state.
#[derive(Clone)]
pub struct Sha256Hasher {
    state: Sha256,
    bytes: u64,
    limit: u64,
}

impl Sha256Hasher {
    /// Starts an empty stream, capping the configured limit at 2^61 - 1 bytes.
    pub fn new(limits: HashLimits) -> Self {
        Self {
            state: Sha256::new(),
            bytes: 0,
            limit: limits.effective_max_bytes(),
        }
    }

    /// Appends a complete chunk, or leaves all state unchanged on failure.
    ///
    /// Empty chunks are valid even at the limit. Checks precede hashing, so
    /// a failed update never hashes a partial chunk or corrupts prior evidence.
    pub fn update(&mut self, input: &[u8]) -> Result<(), HashError> {
        let error = HashError::InputTooLong { limit: self.limit };
        let additional = u64::try_from(input.len()).map_err(|_| error)?;
        let total = self.bytes.checked_add(additional).ok_or(error)?;
        if total > self.limit {
            return Err(error);
        }
        self.state.update(input);
        self.bytes = total;
        Ok(())
    }

    /// Returns the cumulative accepted input bytes.
    pub const fn bytes_hashed(&self) -> u64 {
        self.bytes
    }

    /// Consumes the stream and returns its digest, including for empty input.
    pub fn finish(self) -> Sha256Digest {
        Sha256Digest(self.state.finalize().into())
    }
}

#[cfg(test)]
mod tests {
    use super::{HashError, HashLimits, MAX_BYTES, Sha256Hasher};

    #[test]
    fn algorithm_length_boundary_rejects_before_library_arithmetic_can_overflow() {
        let mut hasher = Sha256Hasher::new(HashLimits {
            max_bytes: u64::MAX,
        });
        // Exercise an otherwise impractical bound without allocating exabytes.
        hasher.bytes = MAX_BYTES;
        let before = hasher.clone().finish();
        hasher.update(b"").unwrap();
        assert_eq!(
            hasher.update(b"x"),
            Err(HashError::InputTooLong { limit: MAX_BYTES })
        );
        assert_eq!(hasher.bytes_hashed(), MAX_BYTES);
        assert_eq!(hasher.finish(), before);
    }
}
