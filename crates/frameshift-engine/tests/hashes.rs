//! SHA-256 contracts through the engine facade, independent of library types.

use std::collections::BTreeSet;

use frameshift_engine::foundation::{HashError, HashLimits, Sha256Digest, Sha256Hasher};

fn hash(input: &[u8]) -> Sha256Digest {
    Sha256Digest::hash(input, HashLimits::default()).unwrap()
}

#[test]
fn standard_known_answers_cover_empty_short_padding_and_long_inputs() {
    let vectors: &[(&[u8], &str)] = &[
        (b"", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        (b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ];
    for &(input, hex) in vectors {
        assert_eq!(format!("{:x}", hash(input)), hex);
        assert_eq!(hash(input).to_string(), format!("sha256:{hex}"));
    }
    assert_eq!(
        format!("{:x}", hash(&vec![b'a'; 1_000_000])),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn binary_bom_line_endings_and_unicode_are_hashed_without_normalization() {
    // Binary expected value independently checked with .NET SHA256.HashData.
    let binary: Vec<u8> = (0..=255).collect();
    assert_eq!(
        format!("{:x}", hash(&binary)),
        "40aff2e9d2d8922e47afd4648e6967497158785fbd1da870e7110266bf944880"
    );
    let variants: &[&[u8]] = &[
        b"x",
        b"x\n",
        b"x\r\n",
        b"\xef\xbb\xbfx",
        b"x\0",
        "é".as_bytes(),
        "e\u{301}".as_bytes(),
        b"\xff",
    ];
    let identities: BTreeSet<_> = variants.iter().map(|input| hash(input)).collect();
    assert_eq!(identities.len(), variants.len());
}

#[test]
fn streaming_boundaries_and_cloned_prefixes_preserve_byte_identity() {
    let input: Vec<u8> = (0..4099).map(|i| (i % 256) as u8).collect();
    let expected = hash(&input);
    for width in [1, 2, 7, 55, 56, 63, 64, 65, 127, 128, 129, 4099] {
        let mut stream = Sha256Hasher::new(HashLimits {
            max_bytes: input.len() as u64,
        });
        for chunk in input.chunks(width) {
            stream.update(b"").unwrap();
            stream.update(chunk).unwrap();
        }
        assert_eq!(stream.bytes_hashed(), input.len() as u64);
        assert_eq!(stream.finish(), expected);
    }
    let mut prefix = Sha256Hasher::new(HashLimits::default());
    prefix.update(b"a").unwrap();
    let mut branch = prefix.clone();
    prefix.update(b"bc").unwrap();
    branch.update(b"different").unwrap();
    assert_eq!(prefix.finish(), hash(b"abc"));
    assert_eq!(branch.finish(), hash(b"adifferent"));
}

#[test]
fn zero_exact_and_cumulative_limits_reject_whole_chunks_without_state_loss() {
    let mut empty = Sha256Hasher::new(HashLimits { max_bytes: 0 });
    assert_eq!(empty.bytes_hashed(), 0);
    empty.update(b"").unwrap();
    assert_eq!(
        empty.update(b"x"),
        Err(HashError::InputTooLong { limit: 0 })
    );
    assert_eq!(empty.finish(), hash(b""));

    let mut stream = Sha256Hasher::new(HashLimits { max_bytes: 3 });
    stream.update(b"a").unwrap();
    assert_eq!(
        stream.update(b"bcd"),
        Err(HashError::InputTooLong { limit: 3 })
    );
    assert_eq!(stream.bytes_hashed(), 1);
    stream.update(b"bc").unwrap();
    stream.update(b"").unwrap();
    assert_eq!(
        stream.update(b"d"),
        Err(HashError::InputTooLong { limit: 3 })
    );
    assert_eq!(stream.bytes_hashed(), 3);
    assert_eq!(stream.finish(), hash(b"abc"));
    assert_eq!(
        Sha256Digest::hash(b"abc", HashLimits { max_bytes: 3 }).unwrap(),
        hash(b"abc")
    );
    assert_eq!(
        Sha256Digest::hash(b"abc", HashLimits { max_bytes: 2 }),
        Err(HashError::InputTooLong { limit: 2 })
    );
    assert_eq!(HashLimits::default().max_bytes, (1_u64 << 61) - 1);
}

#[test]
fn canonical_round_trip_and_raw_bytes_have_one_spelling() {
    for byte in 0..=255 {
        let bytes = [byte; 32];
        let digest = Sha256Digest::from_bytes(bytes);
        let canonical = digest.to_string();
        assert_eq!(canonical.len(), 71);
        assert_eq!(canonical.parse::<Sha256Digest>().unwrap(), digest);
        assert_eq!(digest.as_bytes(), &bytes);
        assert_eq!(format!("{digest:?}"), canonical);
        assert_eq!(format!("{digest:x}"), format!("{byte:02x}").repeat(32));
    }
    let ascending: Vec<_> = (0..=255)
        .map(|b| Sha256Digest::from_bytes([b; 32]))
        .collect();
    let sorted: BTreeSet<_> = ascending.iter().rev().copied().collect();
    assert_eq!(sorted.into_iter().collect::<Vec<_>>(), ascending);
    let first = Sha256Digest::from_bytes([0; 32]);
    let mut last_byte = [0; 32];
    last_byte[31] = 1;
    assert!(first < Sha256Digest::from_bytes(last_byte));
}

#[test]
fn parsing_rejects_noncanonical_text_with_input_free_errors_and_precedence() {
    let valid = hash(b"abc").to_string();
    for input in [
        "".to_owned(),
        "x".repeat(70),
        "x".repeat(72),
        format!(" {valid}"),
        format!("{valid}\n"),
    ] {
        assert_eq!(input.parse::<Sha256Digest>(), Err(HashError::InvalidLength));
    }
    for prefix in ["SHA256:", "sha512:", "sha256-", "secret:"] {
        assert_eq!(
            format!("{prefix}{}", "g".repeat(64)).parse::<Sha256Digest>(),
            Err(HashError::InvalidPrefix)
        );
    }
    for offset in 7..71 {
        for byte in 0..=127 {
            let mut bytes = valid.as_bytes().to_vec();
            bytes[offset] = byte;
            let input = String::from_utf8(bytes).unwrap();
            if byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) {
                assert!(input.parse::<Sha256Digest>().is_ok());
            } else {
                assert_eq!(
                    input.parse::<Sha256Digest>(),
                    Err(HashError::InvalidHex { offset })
                );
            }
        }
    }
    let unicode = format!("sha256:é{}", "0".repeat(62));
    assert_eq!(
        unicode.parse::<Sha256Digest>(),
        Err(HashError::InvalidHex { offset: 7 })
    );
    for error in [
        HashError::InvalidLength,
        HashError::InvalidPrefix,
        HashError::InvalidHex { offset: 7 },
        HashError::InputTooLong { limit: 3 },
    ] {
        for rendered in [error.to_string(), format!("{error:?}")] {
            assert!(!rendered.contains("secret"));
            assert!(!rendered.contains('\u{1b}'));
        }
    }
}
