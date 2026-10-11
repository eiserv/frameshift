# Implementation Status and Text Contracts

**Status: initial M0 implementation.** This is the authoritative map of existing code and its current contracts. The architecture and migration documents describe the remaining product design. M0 is incomplete: neither E1 nor E2 has passed.

## 1. Tooling and code map

- `rust-toolchain.toml` pins Rust 1.98.0, rustfmt, and Clippy (`D-032`).
- The root `Cargo.toml` defines an edition 2024 workspace. Packages are unpublished and use Cargo's default version 0.0.0. `Cargo.lock` is committed. No external crates are used.
- `crates/frameshift-core` is the framework-independent foundation. It currently provides immutable UTF-8 source text, byte spans, derived positions, explicit size limits, and typed text errors.
- `crates/frameshift-transform` implements pure span rewriting and byte-region provenance. It depends only on the foundation.
- `crates/frameshift-engine` is the public facade for these implemented text APIs. Future CLI code will consume it rather than lower layers. It does not yet expose the project pipeline.
- `crates/frameshift-core/tests/text.rs` tests source preservation, positions, and rejected inputs. `crates/frameshift-engine/tests/rewriting.rs` tests rewriting through the public facade.
- `.gitattributes` fixes LF checkout endings for workspace Rust, Markdown, and TOML files and Cargo.lock. It does not normalize the analyzed text held by the library. Fixture-specific Git attributes will be specified when on-disk fixtures are added.
- There is no binary or CLI yet. No parser, project discovery, migration rule, filesystem executor, CI workflow, or release configuration exists.

The workspace forbids unsafe code, requires public API documentation, and denies Clippy's `wildcard_enum_match_arm` lint. None of the implemented library functions access the filesystem, environment, clock, network, or external processes.

## 2. Source text and positions

`SourceText` takes ownership of a `String` or UTF-8 byte vector and keeps it immutable. It never changes encoding, BOMs, whitespace, or line endings. Invalid UTF-8 is rejected; errors contain numeric offsets rather than source content.

`Span` is a half-open byte range, with zero-based start and exclusive end. Its constructor rejects reversed ranges. Slicing and position lookup additionally validate bounds and UTF-8 scalar boundaries. EOF is a valid offset, including in an empty source.

Positions follow `D-029`: byte offsets are canonical; line and column numbers are one-based; columns count Unicode scalar values. A tab, a combining mark, or a BOM counts as one scalar, without tab expansion or grapheme grouping. LF, CRLF, CR, U+2028, and U+2029 terminate lines. CRLF is one terminator. The offset between CR and LF remains on the preceding line, and its column includes the CR. A trailing terminator creates an empty final line.

The default source limit is 16 MiB. Callers pass limits explicitly and may lower or raise it. The compact index imposes an absolute ceiling of `u32::MAX - 1` source bytes so one-based positions remain representable. Size checks precede UTF-8 validation and index allocation. Index reservation failures return a typed error.

Index construction is linear in source bytes. Line starts and checkpoints every 128 scalars support position lookup with two binary searches and a scan of at most 128 scalars, including EOF. This prevents repeated position queries on a long line from becoming quadratic.

## 3. Span rewriting

`apply_edits` takes an immutable `SourceText`, a borrowed set of edits, and explicit `EditLimits`. It returns rewritten text, a partition of its bytes into provenance regions, and a record of every applied edit. It never selects transformations or parses source. Callers must eventually obtain edits from parsed structure and verified rules, not text search.

Each edit has a half-open original-source span, UTF-8 replacement text, and an `EditOrder` pair (operation order, then edit order). Empty spans insert; empty replacements delete. All ordering pairs in one invocation must be unique, including at different offsets. The caller derives these pairs deterministically, never from worker completion order.

Edits are ordered by source offset, with insertions before nonempty replacements at the same start offset. Insertions sharing an offset are ordered by `EditOrder`. At adjacent replacement boundaries, insertions follow the preceding replacement and precede the following replacement. Insertions strictly inside a replaced range are conflicts. Overlapping nonempty ranges, including identical replacements, are conflicts. Invalid bounds, partial UTF-8 scalars, duplicate ordering pairs, and conflicts return typed errors with no partial result.

Validation is deterministic: edit count first, duplicate ordering pairs in pair order, then span validity and conflicts in source order, and final output size. All validation precedes output allocation and emission. Original text stays unchanged on success and failure. Bytes outside edited spans are copied exactly; replacements are emitted exactly as supplied. There is no newline, indentation, or escaping policy in this low-level edit core. Those belong to future rule and printer code.

The default limits are 100,000 edits and 64 MiB of output per invocation. Callers pass limits explicitly. Output size means final size after deletions and replacements. Byte arithmetic is checked for overflow. Storage is reserved fallibly before emission, including edit indexes and provenance. The algorithm sorts edits and scans the source once for emission, with O(source bytes + replacement bytes + edits * log(edits)) time.

Nonempty provenance regions cover every output byte exactly once. Copied regions refer to their exact source spans; replacement regions refer to the responsible ordering pair and consumed source span. Replacement provenance does not imply equivalence. Deletions emit no region but remain in the applied-edit list, with an empty output span. This is per-text provenance only: rule IDs, file identities, and manifest integration remain future work.

## 4. Local validation

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

The suite contains 16 contract tests, run locally on Windows x86_64. It includes all 24 permutations of a four-edit example and all 225 pairs of half-open spans in a four-byte source, checking conflicts, stable output, complete source accounting, and output provenance. None of these tests execute source projects. Cross-platform validation and the complete E1/E2 parser, printer, throughput, memory, fuzzing, end-to-end provenance, and Astro build gates remain outstanding. No automated crate-boundary check exists yet; it is planned with M1 CI. Tests of primitives do not demonstrate migration correctness.
