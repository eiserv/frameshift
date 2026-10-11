# Implementation Status and Text Contracts

**Status: initial M0 implementation.** This is the authoritative map of existing code and its current contracts. The architecture and migration documents describe the remaining product design. M0 is incomplete: neither E1 nor E2 has passed.

## 1. Tooling and code map

- `rust-toolchain.toml` pins Rust 1.98.0, rustfmt, and Clippy (`D-032`).
- The root `Cargo.toml` defines an edition 2024 workspace. Packages are unpublished and use Cargo's default version 0.0.0. `Cargo.lock` is committed. No external crates are used.
- `crates/frameshift-core` is the framework-independent foundation. It currently provides immutable UTF-8 source text, byte spans, derived positions, explicit size limits, and typed text errors.
- `crates/frameshift-core/tests/text.rs` tests source preservation, positions, and rejected inputs.
- There is no binary or CLI yet. No parser, project discovery, migration rule, filesystem executor, CI workflow, or release configuration exists.

The workspace forbids unsafe code, requires public API documentation, and denies Clippy's `wildcard_enum_match_arm` lint. No core function accesses the filesystem, environment, clock, network, or external processes.

## 2. Source text and positions

`SourceText` takes ownership of a `String` or UTF-8 byte vector and keeps it immutable. It never changes encoding, BOMs, whitespace, or line endings. Invalid UTF-8 is rejected; errors contain numeric offsets rather than source content.

`Span` is a half-open byte range, with zero-based start and exclusive end. Its constructor rejects reversed ranges. Slicing and position lookup additionally validate bounds and UTF-8 scalar boundaries. EOF is a valid offset, including in an empty source.

Positions follow `D-029`: byte offsets are canonical; line and column numbers are one-based; columns count Unicode scalar values. A tab, a combining mark, or a BOM counts as one scalar, without tab expansion or grapheme grouping. LF, CRLF, CR, U+2028, and U+2029 terminate lines. CRLF is one terminator. The offset between CR and LF remains on the preceding line, and its column includes the CR. A trailing terminator creates an empty final line.

The default source limit is 16 MiB. Callers pass limits explicitly and may lower or raise it. The compact index imposes an absolute ceiling of `u32::MAX - 1` source bytes so one-based positions remain representable. Size checks precede UTF-8 validation and index allocation. Index reservation failures return a typed error.

Index construction is linear in source bytes. Line starts and checkpoints every 128 scalars support position lookup with two binary searches and a scan of at most 128 scalars, including EOF. This prevents repeated position queries on a long line from becoming quadratic.

## 3. Local validation

From the repository root, with the pinned toolchain installed:

```text
cargo build --workspace --offline --locked
cargo test --workspace --offline --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
```

These commands operate on Frameshift's own code. They do not run any analyzed project's code. Installing the toolchain is a separate development setup operation that may require network access:

```text
rustup toolchain install 1.98.0 --profile minimal --component rustfmt --component clippy
```

Tests have been run locally on Windows x86_64. Cross-platform validation and the E1/E2 throughput, memory, fuzzing, provenance, and Astro build gates remain outstanding. Tests of primitives do not demonstrate migration correctness.
