//! SHA-256 facade contracts; cryptographic known answers live in facade tests.

use frameshift_engine::foundation::{HashError, HashLimits, Sha256Digest, Sha256Hasher};

/// Checks chunk identity, inclusive limits, failed updates, and digest spelling.
pub fn run(input: &[u8]) -> Result<(), String> {
    let full = Sha256Digest::hash(input, HashLimits::default())
        .map_err(|error| format!("bounded fuzz input failed hashing: {error}"))?;
    let width = 1 + usize::from(input.first().copied().unwrap_or(0));
    let mut stream = Sha256Hasher::new(HashLimits::default());
    for chunk in input.chunks(width) {
        stream.update(b"").map_err(|e| e.to_string())?;
        stream.update(chunk).map_err(|e| e.to_string())?;
    }
    if stream.bytes_hashed() != input.len() as u64 || stream.finish() != full {
        return Err("chunk boundaries changed the digest or byte count".to_owned());
    }
    for limit in [0, input.len() / 2, input.len()] {
        let limits = HashLimits {
            max_bytes: limit as u64,
        };
        let actual = Sha256Digest::hash(input, limits);
        let expected = if input.len() <= limit {
            Ok(full)
        } else {
            Err(HashError::InputTooLong {
                limit: limit as u64,
            })
        };
        if actual != expected {
            return Err("one-shot hash did not respect its inclusive byte limit".to_owned());
        }
        let mut limited = Sha256Hasher::new(limits);
        limited.update(&input[..limit]).map_err(|e| e.to_string())?;
        if limited.update(b"x")
            != Err(HashError::InputTooLong {
                limit: limit as u64,
            })
            || limited.bytes_hashed() != limit as u64
            || limited.finish()
                != Sha256Digest::hash(&input[..limit], limits).map_err(|e| e.to_string())?
        {
            return Err("failed update changed the prefix digest or byte count".to_owned());
        }
    }
    let canonical = full.to_string();
    if canonical.parse::<Sha256Digest>() != Ok(full)
        || Sha256Digest::from_bytes(*full.as_bytes()) != full
    {
        return Err("digest serialization changed its raw bytes".to_owned());
    }
    if let Ok(text) = std::str::from_utf8(input) {
        let valid = text.strip_prefix("sha256:").is_some_and(|hex| {
            hex.chars().count() == 64 && hex.chars().all(|ch| "0123456789abcdef".contains(ch))
        });
        let parsed = text.parse::<Sha256Digest>();
        if parsed.is_ok() != valid
            || parsed != text.parse::<Sha256Digest>()
            || parsed.is_ok_and(|digest| digest.to_string() != text)
        {
            return Err("digest parser disagrees with canonical grammar or repetition".to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn generated_binary_and_digest_spellings_preserve_contracts() {
        for length in 0..=256 {
            let input: Vec<u8> = (0..length).map(|i| i as u8).collect();
            run(&input).unwrap();
        }
        for target in crate::targets::Target::Hashes.seeds() {
            run(&target).unwrap();
        }
        for width in [55, 56, 63, 64, 65, 127, 128, 129, 4096] {
            run(&vec![0xff; width]).unwrap();
        }
    }
}
