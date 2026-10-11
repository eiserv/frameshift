# Implementation Status and Source Contracts

**Status: M0 in progress.** This is the authoritative map of existing code and its current contracts. The architecture and migration documents describe the remaining product design. M0 is incomplete: neither E1 nor E2 has passed.

## 1. Tooling and code map

- `rust-toolchain.toml` pins Rust 1.98.0, rustfmt, and Clippy (`D-032`).
- The root `Cargo.toml` defines an edition 2024 workspace. Packages are unpublished and use Cargo's default version 0.0.0. `Cargo.lock` is committed. No external crates are used.
- `crates/frameshift-core` is the framework-independent foundation. It currently provides immutable UTF-8 source text, byte spans, derived positions, explicit size limits, and typed text errors.
- `crates/frameshift-syntax` provides parser-directed JavaScript trivia and strict-string scanners for E1. It depends only on the foundation and has no framework knowledge.
- `crates/frameshift-transform` implements pure span rewriting and byte-region provenance. It depends only on the foundation.
- `crates/frameshift-engine` is the public facade for these implemented text and lexical APIs. Future CLI code will consume it rather than lower layers. It does not yet expose the project pipeline.
- `crates/frameshift-core/tests/text.rs` tests source preservation, positions, and rejected inputs. `crates/frameshift-engine/tests/rewriting.rs` and `tests/lexical.rs` test rewriting and lexical contracts through the public facade.
- `.gitattributes` fixes LF checkout endings for workspace Rust, Markdown, and TOML files and Cargo.lock. It does not normalize the analyzed text held by the library. Fixture-specific Git attributes will be specified when on-disk fixtures are added.
- There is no binary or CLI yet. No complete lexer, syntax tree, parser, project discovery, migration rule, filesystem executor, CI workflow, or release configuration exists.

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

## 4. Parser-directed JavaScript lexical primitives

`scan_js_trivia` and `scan_js_string` take immutable `SourceText`, a caller-selected byte offset, and explicit `LexicalLimits`. Offsets are validated before inspection. The caller must select the correct JavaScript lexical context. These functions cannot distinguish JavaScript from JSX text, JSX attributes, regular expressions, or template literal bodies, and must not be invoked to guess that context. They do not validate a file or make it eligible for transformation.

The scanner contract follows ECMAScript lexical grammar sections [12.2 to 12.4 and 12.9.4](https://tc39.es/ecma262/multipage/ecmascript-language-lexical-grammar.html), checked on 2026-10-12. Whitespace is the explicit set TAB, VT, FF, SPACE, U+00A0, U+1680, U+2000 through U+200A, U+202F, U+205F, U+3000, and U+FEFF. Line terminators are CR, LF, U+2028, and U+2029. Rust's broader Unicode whitespace predicate is not used. U+0085, U+180E, and U+200B are not consumed.

Trivia scanning consumes consecutive whitespace and `//` or `/* */` comments. It returns the complete consumed span, comments in source order with exact delimiter-inclusive spans, and a flag indicating any line terminator, including inside block comments. Line comment spans exclude their terminating line break. Block comments do not nest. EOF is accepted for a line comment, but an unclosed block comment returns an error at its opening delimiter. A slash that does not start a comment, hashbang comments, and legacy HTML comments remain unconsumed for future lexical-goal handling. A non-trivia input produces an empty consumed span, not a syntax-validity claim.

String scanning requires a single or double quote and returns the delimiter-inclusive span of one strict-mode ECMAScript string literal. Bytes are neither copied nor decoded. It accepts character escapes, `\0` without a following decimal digit, `\xHH`, `\uHHHH`, `\u{...}` up to 0x10FFFF, and escaped line continuations (including CRLF as one continuation). Unicode escapes can contain surrogate code points because the result is an original-source span, not a Rust string value. Raw U+2028 and U+2029 are legal string characters; raw CR and LF are rejected. Legacy decimal escapes return a distinct unsupported error. JSX attributes, JSON strings, and template literals require different future scanners.

Default limits are 100,000 retained comments per trivia invocation and 1 MiB per string, including its delimiters. `SourceText` separately bounds total source bytes. String bytes are checked before each scalar is consumed; comment count is checked before scanning another comment. Invalid starting offsets take precedence over lexical checks. Missing quotes, unclosed strings/comments, malformed escapes, exceeded limits, and allocation failures return typed errors with numeric offsets and no source snippets or partial result. String-limit errors identify the first scalar that would exceed the bound; invalid escape errors identify the backslash. These are local scan limits, not a file-wide token/comment budget. A future parser must enforce aggregate limits.

Both scanners are iterative and linear in consumed bytes. String scans allocate no storage; trivia retains only bounded comment records and reserves storage fallibly. Original source, BOM, comments, and line endings remain unchanged. The separate syntax crate prepares the architectural syntax layer for parser-controlled lexical goals and later AST arenas (`D-033`); neither is implemented yet. P-002 remains Proposed until full E1 evidence exists.

## 5. Local validation

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

The suite contains 27 contract tests, run locally on Windows x86_64. It includes all 24 permutations of a four-edit example and all 225 pairs of half-open spans in a four-byte source, checking conflicts, stable output, complete source accounting, and output provenance. Lexical tests cover whitespace and comment boundaries, newline evidence, strict-string escapes, exact error offsets, limits, and an edit driven by a scanned string span. A deterministic robustness test exercises all 7,381 strings of length zero through four over a nine-scalar alphabet, at every scalar boundary and EOF. This is bounded generated testing, not coverage-guided fuzzing. None of these tests execute source projects. Cross-platform validation and the complete E1/E2 parser, printer, throughput, memory, fuzzing, end-to-end provenance, and Astro build gates remain outstanding. No automated crate-boundary check exists yet; it is planned with M1 CI. Tests of primitives do not demonstrate migration correctness.
