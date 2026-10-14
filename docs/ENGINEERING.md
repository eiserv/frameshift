# Engineering

**Status: guidance and decisions with M0 complete and M1 in progress.** Rust 1.98.0, rustfmt, and Clippy are pinned (`D-032`). Immutable source text, position primitives, parser-directed JavaScript scanners, regular-expression validation, identifier classification from generated Unicode 18.0.0 tables, a JavaScript parser with an arena tree for complete Script and Module source text, the span edit core, and the Astro component printer exist, with no external crates. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for actual modules, limits, and commands. This document records the implementation language decision, defines the build-versus-adopt policy for dependencies, sets performance goals, and establishes engineering conventions for future work. Rust is the accepted primary language (`D-018`). Other languages may be used for specific, important tasks under the rules in section 2.7 (`D-019`). Other recommendations are recorded as "Proposed" in [DECISIONS.md](DECISIONS.md) until accepted.

## 1. Engineering principles

- **Quality over effort (`D-020`).** Effort, time investment, and other economic considerations are never, on their own, reasons to choose a lower-quality solution. Choices are judged on correctness, safety, maintainability, performance, and professionalism. Structures must remain understandable, and architectural preparation must have a documented rationale (`D-033`).
- **Correctness and safety before speed of delivery.** There are no deadlines. Work that improves long-term correctness is worth its cost.
- **Forward-looking architecture (`D-033`).** Every increment must account for the documented V1 to V3 evolution. Anticipate subsystem boundaries, extension needs, and compatibility. Record what future requirements a design supports and what remains unresolved. Keep structures understandable (KISS), preserve experiment gates, and label future contracts as planned until implemented.
- **Determinism is a code property, not a test property.** Code must be written so that nondeterminism cannot arise (section 6), not just tested for it.
- **Fail closed.** Code paths that meet something unexpected report it. They never fall through to a default that looks like success.
- **Minimal dependencies, deliberately chosen.** See section 3.
- **Performance awareness.** Measure, budget, and avoid accidental complexity. Do not micro-optimize without evidence.

## 2. Implementation language

### 2.1 Workload characteristics

The choice follows from what Frameshift actually does:

| Aspect | Characteristic |
| --- | --- |
| Input size | Typical target projects contain 50 to 2,000 source files (roughly 0.5 to 20 MB of code). Large repositories in later stages may contain tens of thousands of files. |
| CPU profile | Lexing and parsing dominate the engine's own time. Graph construction is near-linear. Rule evaluation is light. |
| Parallelism | Per-file work is embarrassingly parallel. Global steps are sequential but cheap. |
| Latency | Batch CLI runs. Pause times are irrelevant. Total wall time and peak memory matter. |
| End-to-end time | Dominated by external verification (package installation, framework builds), often by orders of magnitude. |
| Code character | Compiler-like: large tree structures, many node kinds, transformations, printers. A type system that helps with closed sets of variants is valuable. |
| Trust | Parses untrusted input, so memory safety is mandatory. |
| Distribution | Self-contained binaries for six platform targets (see [RELEASES.md](RELEASES.md)). |
| Lifetime | A long-lived project maintained by a small team plus agents. Stability of the language and toolchain matters. |

### 2.2 Candidates and evaluation

Ratings: **Strong**, **Good**, **Adequate**, **Weak**, **Poor**, judged for Frameshift's workload, not in general.

| Criterion | Go | Rust | Zig | C | C++ | Haskell | OCaml | TypeScript on Node or Bun |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Execution performance | Good | Strong | Strong | Strong | Strong | Adequate | Good | Adequate |
| Memory efficiency | Good | Strong | Strong | Strong | Strong | Adequate | Good | Weak |
| Memory safety | Strong | Strong | Weak | Poor | Weak | Strong | Strong | Strong |
| Reliability tooling (race detection, fuzzing, sanitizers) | Strong | Strong | Adequate | Adequate | Adequate | Good | Good | Good |
| Reusable JS/TS parser libraries | Weak | Strong | Poor | Adequate (CST only) | Adequate (CST only) | Weak | Weak | Strong |
| Standard library coverage (reduces dependencies) | Strong | Adequate | Adequate | Weak | Adequate | Adequate | Adequate | Good |
| Cross-compilation | Strong | Adequate | Strong | Weak | Weak | Weak | Weak | Not applicable |
| Self-contained binary distribution | Strong | Good | Strong | Adequate | Adequate | Weak | Adequate | Weak |
| Interoperability (C ABI, WebAssembly) | Adequate | Strong | Strong | Strong | Strong | Adequate | Adequate | Adequate |
| Language and toolchain stability | Strong | Good | Weak | Strong | Good | Good | Good | Adequate |
| Toolchain complexity | Strong | Good | Good | Weak | Weak | Adequate | Adequate | Adequate |
| Development ergonomics | Strong | Good | Adequate | Weak | Adequate | Good | Good | Strong |
| Suitability for AST transformation | Good | Strong | Adequate | Weak | Adequate | Strong | Strong | Good |

Evidence considered (state as of October 2026):

- **TypeScript 7.0** (mid-2026) ships Microsoft's native port of the TypeScript compiler, written in Go. Microsoft reports roughly 8x to 12x faster full builds than the JavaScript implementation. Its Go packages are internal, and version 7.0 has no stable programmatic API.
- **esbuild**, written in Go, shows that very fast JavaScript and TypeScript parsing is achievable in Go with parallelism. Its parser is an internal package designed for bundling and minification.
- **Astro 7** (2026) replaced its Go-based `.astro` compiler with a Rust implementation built on oxc and Lightning CSS. This illustrates Rust's main advantage: reusable, high-quality components from the JavaScript tooling ecosystem.
- **oxc** (Rust) offers a fast, conformant parser and semantic analysis, but its crates and npm packages are on a pre-1.0 version line with frequent releases, so its API can change often.
- **Go 1.26** made the "Green Tea" garbage collector the default. Its release notes estimate 10 to 40 percent less garbage collection overhead for programs that use the collector heavily.
- **Zig 0.16** (April 2026) is pre-1.0. Breaking changes continue, and there is no committed date for a stable language.

### 2.3 Decision: Rust as the primary language

**Accepted (`D-018`): Rust is Frameshift's primary implementation language for the engine and the CLI.** The decision covers all stages, V1 to V3. No later change of language or rewrite is planned. Other languages may be used for specific, important tasks under the rules in section 2.7 (`D-019`).

Reasoning, judged against the full lifetime of the project rather than V1 alone:

1. **Frameshift is a multi-language program transformation system.** Across its stages it needs many parsers, syntax trees with hundreds of node kinds, semantic analysis, rules, and printers. Algebraic data types with exhaustive pattern matching make the compiler report every variant that is not handled, which directly supports fail-closed design (section 7.3). This advantage grows with every supported language and node kind.
2. **Precedent.** Tools that search and transform code structurally across many languages are predominantly written in Rust (ast-grep, GritQL, difftastic) or OCaml. The flagship Go tools in this area, esbuild and the TypeScript 7 compiler, each handle a single language.
3. **Ecosystem.** tree-sitter, should it be adopted for long-tail languages later, integrates natively. The Rust JavaScript tooling ecosystem (oxc, Lightning CSS, Biome) is available as test oracles or, after a decision record, behind facades. Astro itself moved its compiler to Rust in 2026.
4. **Performance and memory.** No garbage collector and full control over data layout. This matters for large monorepos in V2 and V3.
5. **Safety.** Memory safety and data-race freedom without a garbage collector. `unsafe` code can be forbidden in all Frameshift crates.
6. **Embedding.** A C ABI and WebAssembly support keep future frontends and integrations open without a second implementation.
7. **Stability.** Stable Rust guarantees backward compatibility, and editions allow the language to evolve without breaking existing code.

Accepted costs (all of them quality-neutral or addressed by conventions):

- The standard library lacks JSON, argument parsing, and cryptographic hashing. These are built or adopted deliberately (section 3.5).
- Building for every platform needs native CI runners per operating system (see [RELEASES.md](RELEASES.md)).
- Longer compile times and a steeper learning curve.
- Graph-shaped data requires arena and index designs instead of pointer graphs.

**Relation to the earlier recommendation.** The first version of this document recommended Go (former `P-001`): for its standard library, single-host cross-compilation, simplicity, and proven performance for single-language compilers. A re-evaluation against the complete V1 to V3 scope gave more weight to type-system support for many variant kinds and to the multi-language ecosystem. Go remains the documented alternative, and its evaluation is kept in section 2.4.

### 2.4 Go, evaluated in detail

Go was treated as a first-class candidate (`D-015`) and was the initial recommendation. This evaluation is kept as part of the record behind `D-018`.

| Question | Assessment |
| --- | --- |
| Fast development of reliable cross-platform CLIs | Strong. Small language, consistent formatting (`gofmt`), fast compilation, a standard library that covers file handling, processes, JSON, and text. |
| Cross-compilation | Strong. With CGO disabled, one toolchain on one host produces binaries for every planned target by setting `GOOS` and `GOARCH`, including `windows/arm64` and both macOS architectures. |
| Self-contained binaries | Strong. With CGO disabled, Linux binaries are fully static. On macOS and Windows, Go links only against system libraries, as the platforms require. |
| Goroutines and concurrency | Strong fit for parallel per-file parsing: bounded worker pools, channels, and cancellation through contexts. |
| Runtime performance and memory | Good. Compiled native code. Typically more memory than Rust because of collector headroom. Go 1.26 made the Green Tea collector the default, which reduces collector overhead. |
| Garbage collection overhead compared with Rust, Zig, C, and C++ | Real but acceptable for a batch tool. Those languages give deterministic memory use and lower peak memory. |
| Parsing, transformation, compiler-like workloads | Proven by the TypeScript native compiler and esbuild. The main weakness is the lack of sum types and exhaustive pattern matching, which makes tree code more verbose and lets a missing case compile. This weakness grows with the number of supported languages. |
| JavaScript, TypeScript, JSX, and TSX parsing infrastructure | Weak as reusable libraries. esbuild's and the TypeScript compiler's parsers are internal packages. Community forks exist. |
| Multi-language parsing | tree-sitter bindings require CGO. A pure-Go tree-sitter runtime (gotreesitter) exists, but it is young, pre-1.0, and apparently maintained by a single person. |
| Foreign-function interoperability and CGO | cgo adds a C toolchain for every target, breaks single-host cross-compilation, and complicates reproducible builds. |
| Long-term maintainability and toolchain simplicity | Strong. The Go 1 compatibility promise and one toolchain for building, testing, fuzzing, profiling, and vulnerability scanning. |

### 2.5 Rust, evaluated in detail

- **Strengths:** top-tier performance and memory efficiency without a garbage collector. Memory safety with data-race freedom. Algebraic data types with exhaustive pattern matching, which suit syntax trees, IR variants, and rule results especially well. Strong WebAssembly and C ABI support. The richest ecosystem of JavaScript tooling and multi-language parsing components.
- **Weaknesses and how Frameshift addresses them:**
  - *Standard library gaps* (JSON, argument parsing, hashing, benchmarking and fuzzing harnesses): handled per component in section 3.5.
  - *Cross-compilation:* native CI runners for every platform. GitHub provides them for all planned targets, including ARM64 in private repositories.
  - *Compile times:* a deliberate crate structure, and incremental builds in development.
  - *Ownership and graph-shaped data:* arenas and indices, the established design in Rust compilers and in oxc.
  - *Learning curve:* documented conventions and reviews.
- **Existing parsers.** oxc is fast and conformant, but its crates remain pre-1.0 (around version 0.14x in 2026) and its releases regularly contain breaking AST changes. It is therefore not an accepted foundation dependency. Purpose-built parsing is accepted (`P-002`), because E1 passed (`D-047`). oxc may serve as a development-time test oracle, or behind a Frameshift-owned facade if a decision record justifies it.
- **Rust-specific rules for Frameshift crates:**
  - `unsafe` code is forbidden unless a decision record justifies a specific use.
  - Matches over closed enumerations in core code use no wildcard arms, so adding a variant forces every match to be revisited (enforced by a lint).
  - Collections whose iteration order can affect output are ordered collections, or hash maps with a fixed, deterministic hasher.
  - Panics never cross the engine API (section 7.2).
  - Formatting and linting with the toolchain's standard tools are mandatory in CI.

### 2.6 Other candidates

- **Zig.** Excellent control, performance, and cross-compilation. However, it is pre-1.0 with ongoing breaking changes, it is not memory-safe (safety checks catch some errors in checked build modes), and it has no reusable JS/TS parser. Too risky as a foundation for a long-lived project today.
- **C.** Maximum portability and performance, but memory-unsafe parsing of untrusted input is a severe liability. Rejected as a primary language.
- **C++.** Same memory-safety concerns as C, plus complex build systems and toolchains. Rejected as a primary language.
- **Assembly.** Only justified for a measured, specialized hotspot (for example, vectorized byte scanning in a lexer) after profiling shows that high-level code cannot meet a budget. Rust allows targeted intrinsics and inline assembly if that ever happens.
- **Haskell.** Strong for language processing: algebraic data types, parser combinators, pure transformations. Weak static distribution (especially on macOS), weak JS/TS tooling, and garbage collection with laziness-related space leaks. Not selected as the primary language.
- **OCaml.** An excellent fit for the domain, with a long track record in program analysis tools. Weaker Windows support and cross-compilation, and a smaller ecosystem and contributor pool. The second-best candidate, not selected.
- **TypeScript on Node.js or Bun.** The best semantic tooling for the source language (the TypeScript compiler API, Babel). However, it requires a JavaScript runtime or large bundled executables, its dependency culture conflicts with the dependency policy, and it is slower. Rejected for the engine.

### 2.7 Additional languages for specific tasks

**Accepted (`D-019`).** Rust is the primary language, but Frameshift's long-term architecture is not fully known. Other languages may be used for specific, important tasks when they provide a demonstrable advantage in correctness, quality, safety, performance, or capability that Rust cannot provide equally well. Effort and time investment are not arguments against such a choice (`D-020`).

Every additional language must meet all of the following:

1. **Decision record.** An entry in [DECISIONS.md](DECISIONS.md) names the task, explains why Rust is not the best tool for it, gives evidence (a prototype, a measurement, or an ecosystem fact), and lists the alternatives considered.
2. **Clear boundary.** The component sits behind a defined interface. That is either a C ABI linked into the binary, a WebAssembly module, or a separate process with a versioned, structured protocol. No ad-hoc coupling.
3. **Safety.** Code that processes untrusted input is memory-safe, or confined (process or WebAssembly isolation). C or C++ code that handles untrusted input is fuzzed and runs under sanitizers in CI.
4. **Distribution quality.** It builds reproducibly with pinned toolchains on every tier 1 platform. The self-contained binary is preserved, unless a recorded decision makes the component an optional external helper.
5. **Determinism and fail-closed behavior across the boundary.** Errors on the other side map to findings or internal errors, never to silent success.
6. **Same engineering standard.** Tests, CI, formatting, linting, and documentation, plus an entry in `AGENTS.md` once the component exists.
7. **Core independence.** Optional components (verification helpers, a future model advisor) never become requirements for inspection, analysis, planning, or migration.

Actual uses: the development-only processor-time helper `tools/frameshift-cputime` in Go (`D-046`), because the Rust standard library has no processor-time clock and Go's reads it without unsafe code or dependencies; and `tools/frameshift-release` (`D-052`), because Go supplies reviewed hashing, compression, and both archive formats in its standard library.

Plausible future cases (examples, not decisions):

- the tree-sitter runtime and generated grammars (C) for long-tail languages in V2 or V3;
- the official compiler or tooling of a framework, run as an opt-in external verification helper;
- SIMD or assembly code for a hotspot proven by profiling;
- a formal specification or verification of rule semantics in a proof assistant or functional language, as development tooling.

Not allowed: a second language for convenience or preference alone, or the same functionality implemented in two languages.

### 2.8 Experiments for core techniques

The language is decided. Two experiments validate the core techniques before milestone M1 (see [ROADMAP.md](ROADMAP.md)), because the parser and tree design shape everything after them.

**E1: parser and tree design in Rust.**

- Scope: a lexer and parser for JavaScript, TypeScript, JSX, and TSX that covers modules, statements, expressions (including arrow functions, async and await, optional chaining, template literals, destructuring), classes, JSX and TSX (including the generic arrow ambiguity), and TypeScript type syntax (parsed and preserved as spans). The arena-based tree representation must be designed for the real implementation, not as a throwaway.
- Corpus: Frameshift's fictional fixtures plus a sample of real-world Next.js App Router files. Real-world files are kept outside the repository, and their licenses are respected.
- Success criteria:
  - At least 99 percent of corpus files parse.
  - Every failure is explained by syntax not yet implemented, never by a design flaw.
  - Span correctness is checked by slicing tests.
  - Throughput of at least 30 MB/s per core. For E1, this and the memory criterion are judged on the development machine; reference-hardware checks of all budgets follow before releases (`D-047`, section 4.3).
  - Peak heap demand of at most 20 times the total source bytes of each corpus with every source text and tree retained (`D-045`). Per-file peaks remain diagnostics; an all-empty corpus has no comparable ratio.
  - Fuzzing for a fixed CPU budget of one CPU-hour per fuzz target (`D-044`) finds no crashes, hangs, or contract violations.
  - A documented review concludes that the arena, visitor, and pattern-matching design is maintainable and extensible to further languages.
- Thresholds are set to expose design problems, not to win benchmarks.

**E2: edit-based rewriting and Astro printing.**

- Scope: the span edit engine and a minimal Astro component printer, applied end to end to three fixtures: a static page with links, a layout with metadata, and a client counter island.
- Success criteria:
  - Comments and formatting are preserved in untouched regions.
  - Output is byte-identical across repeated runs and across operating systems.
  - Provenance records map every output region back to its source.
  - Astro builds the output with the pinned target profile.
  - The result confirms or refutes the assumption that a span-precise AST is enough without a concrete syntax tree (`P-003`).
- Result (2026-10-13): passed on Windows x86_64 and Linux x86_64, with macOS byte identity shown by M1 CI, and `P-003` is accepted (`D-048`, `D-049`, section 11 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).

Later experiments, run before the milestones that depend on them, are listed in [DECISIONS.md](DECISIONS.md): E3 (static evaluation coverage), E4 (static output comparison), and E5 (draft and immutable release flow).

### 2.9 Revisiting the language decision

`D-018` is meant to hold for the lifetime of the project. It is revisited only if a fundamental problem appears that cannot be solved within Rust or through an additional language under section 2.7, and the evidence for it is documented. Any revision requires a decision record with a full migration impact analysis.

### 2.10 Decisions that can safely be deferred

Plugin mechanism (`DF-002`), WebAssembly target (`DF-012`), GUI technology (`DF-006`), concrete foreign-function interfaces (under section 2.7), model-assisted processing (`DF-005`), and code signing (`DF-004`) do not need to be decided now. The engine API and JSON contracts keep these options open.

## 3. Build versus adopt

### 3.1 Dependency categories

| Category | Examples | Policy |
| --- | --- | --- |
| Toolchain | Compiler, linker, standard library | Unavoidable. Pinned to an exact version. Upgraded deliberately. |
| Development tools | Formatters, linters, vulnerability scanners, release tooling, GitHub Actions | Allowed with justification. Never shipped. Pinned (Actions pinned to full commit SHAs). |
| Runtime libraries | Anything linked into the shipped binary | Strongest scrutiny. Default answer is no. Each one needs an accepted dependency decision record. |
| Test-only libraries and data | Assertion helpers, conformance suites, fixture corpora | Moderate scrutiny. Licenses must allow use, and committed third-party data needs notices. |
| External programs at runtime | Package managers, the Astro CLI, browsers | Only for verification, only with explicit authorization, never required for inspection, analysis, planning, or migration. |

### 3.2 Decision criteria

A dependency is evaluated on:

1. **Technical suitability:** does it do exactly what Frameshift needs, without forcing the architecture to bend around it?
2. **Completeness:** does it cover required functionality, including edge cases that matter for correctness?
3. **Performance:** within budget in Frameshift's usage pattern?
4. **Security:** history of vulnerabilities, attack surface, handling of untrusted input.
5. **Maintenance burden:** upgrade frequency, breaking changes, number of transitive dependencies.
6. **API stability:** versioning policy, and whether pre-1.0 churn is acceptable.
7. **Licensing:** permissive licenses (MIT, BSD, Apache-2.0, ISC) are preferred, because the future license is undecided (`DF-003`). Copyleft runtime dependencies require an explicit decision.
8. **Sustainability:** maintainers, governance, funding, and bus factor.
9. **Portability:** builds from source with the pinned toolchains on every tier 1 platform, without extra system libraries.
10. **Control over critical behavior:** if the component decides correctness (parsing, classification, code generation), Frameshift must be able to define and fix its behavior.
11. **Binary size and build impact.**
12. **Exit cost:** how hard it would be to replace later.

### 3.3 Outcomes

| Outcome | When |
| --- | --- |
| Adopt | Suitable on all criteria, especially for development tools |
| Adopt behind a facade | Suitable, but replacement must stay possible. Frameshift code depends only on its own interface. |
| Fork or vendor | Valuable code that no longer fits upstream. Frameshift takes ownership, with license notices. |
| Build | No suitable option, or behavior is critical and Frameshift must control it |
| Defer | Not needed in the current stage |

### 3.4 Guardrails for building

Building is not a license for unbounded work:

- **Specification first.** Each purpose-built component implements a written specification: an external standard (the ECMAScript and CSS syntax specifications, Git's gitignore rules, the npm semver range grammar) or an internal document.
- **Bounded scope.** Implement what Frameshift needs. A YAML parser for lockfiles parses the lockfile subset and rejects everything else explicitly.
- **Conformance testing.** Use public conformance suites where their licenses allow, for example the TypeScript conformance tests (Apache-2.0) or test262, as test inputs. They are not shipped.
- **Fuzzing.** Every parser of untrusted input is fuzzed continuously.
- **Documented limitations.** Unsupported syntax is reported explicitly and never silently mis-parsed.
- **Never build** cryptography, compression, or anything the standard library provides adequately.

### 3.5 Proposed component decisions

Proposals for a Rust implementation. Each runtime adoption still needs its own decision record (section 3.6).

| Component | Category | Proposal | Notes |
| --- | --- | --- | --- |
| JS, TS, JSX, TSX parser | Runtime | Build (E1 passed, `P-002` accepted) | Most critical component. Needs exact spans, comments, error policy, and resource limits. oxc may serve as a development-time test oracle. |
| CSS tokenizer and structural parser | Runtime | Build | Specified by CSS Syntax Level 3. Bounded. Lightning CSS may serve as a test oracle. |
| JSON reading and deterministic writing | Runtime | Build | RFC 8259 is small and precisely specified. Frameshift needs control over key order, formatting, and error positions, and shares the code with JSONC. |
| JSONC | Runtime | Build | Extension of the JSON parser |
| YAML subset for lockfiles | Runtime | Build | General YAML is large and risky. Only the lockfile subset is needed. |
| Yarn classic lockfile | Runtime | Build | Small format |
| npm semver range matching | Runtime | Build | Needed for knowledge base matching. The grammar is well documented. |
| Unicode identifier properties and property value names | Runtime data | Build: generated tables from the pinned Unicode Character Database (`D-036`, `D-037`, implemented) | A standard-library-only development tool generates `ID_Start` and `ID_Continue` ranges and the General_Category and Script value names that regular-expression property escapes accept. Rust's own Unicode predicates use other properties and an unpinned version. Unicode's terms require a notice before distribution. |
| gitignore and glob matching | Runtime | Build | Specified behavior, small |
| SHA-256 hashing | Runtime | Adopt a well-audited implementation (for example from the RustCrypto project) after review | The standard library has no cryptographic hashes, and cryptography is never built in-house |
| Parallelism | Runtime | Build on the standard library (scoped threads, channels) | A small, deterministic worker pool is enough. No work-stealing scheduler needed. |
| Argument parsing | Runtime | Build | Small, and the CLI contract must be exact |
| Terminal detection and color | Runtime | Build on the standard library | Terminal detection is in the standard library. Colors are plain ANSI sequences honoring `NO_COLOR`. |
| Text diff for previews | Runtime | Build if needed | Myers' algorithm |
| Astro component printer and JSX conversion | Runtime | Build (`D-048`, implemented in `frameshift-emit`) | Canonical output with exact provenance and fail-closed conversion; no formatter or Astro tooling runs during migration |
| HTML parsing for static output comparison | Runtime (verification) | Evaluate an established HTML5 parser (for example html5ever) against building (E4, `Q-005`) | The HTML5 parsing algorithm is large but fully specified |
| tree-sitter runtime and grammars | Runtime (V2 or later) | Evaluate when multi-language support starts (`D-019`) | C code compiled into the binary. Needs fuzzing and sanitizer runs. |
| Unit and integration tests | Development | Standard toolchain | |
| Golden-file and snapshot testing | Development | Build | Small, and the update workflow must match [TESTING.md](TESTING.md) |
| Benchmarks | Development | Build: a small standard-library harness with a counting allocator (`D-040`, implemented as `tools/frameshift-bench`). Statistics-based tools are evaluated again for M11. | The standard benchmark attribute is not stable, and criterion does not measure memory |
| Fuzzing | Development | Build: a coverage-guided fuzzer with stable SanitizerCoverage instrumentation, a purpose-built runtime, and a purpose-built mutator (`D-044`, implemented as `tools/frameshift-fuzz`). cargo-fuzz was not adopted, because it needs a nightly toolchain and a C++ build. | Mandatory for every parser of untrusted input. The instrumentation works with the pinned stable toolchain on Windows; Linux and macOS are not yet validated. |
| Processor-time measurement for budget runs | Development | Build in Go with its standard library (`D-046`, implemented as `tools/frameshift-cputime`) | The Rust standard library has no processor-time clock. Go's standard library reads the times of a process tree on Windows and Linux without unsafe code. |
| Formatting and linting | Development | rustfmt and Clippy (part of the toolchain); `gofmt` and `go vet` for the Go helper | Lints are configured, not just defaults |
| Dependency audits and vulnerability scanning | Development | Evaluate cargo-audit and cargo-deny | Also checks licenses and duplicate dependencies |
| Release automation | CI | Release Please action, GitHub CLI | Pinned (see [RELEASES.md](RELEASES.md)) |
| Headless browser for runtime verification and fixture checks | External verification tool and development | User-installed external tool, never bundled. Tool and check protocol decided in M10. | Explicit execution authorization is required for product verification. |

### 3.6 Process

The development-only Go release helper implements archives and checksums without new dependencies (`D-052`). Official CI Actions and the local workflow validator are evaluated and pinned under `D-051`; the exact pins and publication gates are in [RELEASES.md](RELEASES.md).

- Every runtime dependency, and every development tool that affects build output, gets an entry in [DECISIONS.md](DECISIONS.md) with the evaluation against section 3.2.
- Dependencies are pinned. Upgrades are separate commits (`build(deps): ...`) with a changelog review.
- A third-party notices file is generated from the dependencies actually shipped before any public release (see [SECURITY.md](SECURITY.md)).

## 4. Performance

### 4.1 Initial budgets

Reference machine: an 8-core x86_64 or ARM64 laptop from 2023 or later, with an SSD and a warm filesystem cache. These are initial targets, to be refined by E1 and by benchmarks.

| Scenario | Wall time | Peak memory |
| --- | --- | --- |
| `inspect` on a repository with 5,000 files (excluding ignored paths) | under 0.5 s | under 200 MB |
| `analyze` on a project with 1,000 modules (about 8 MB of source) | under 2 s | under 500 MB |
| `analyze` on 10,000 modules (about 80 MB of source; V2 target) | under 15 s | under 3 GB |
| `migrate` without external verification | under 2 times the `analyze` time | comparable to `analyze` |
| Parser throughput (E1) | at least 30 MB/s per core | peak heap demand at most 20 times total corpus source bytes with full retention (`D-045`) |

### 4.2 Practices

- Algorithms over files, nodes, and graph edges must be linear or near-linear. Quadratic behavior requires a written justification.
- Parse each file once. Share immutable results between stages.
- Traverse syntax trees with explicit stacks or loops. Parser limits bound the parser's recursion, not tree depth, because chains such as `a.b.c` are built without nesting; recursion is acceptable only over constructs whose depth a limit bounds, such as markup nesting.
- Intern identifiers and paths. Use compact node representations and indices instead of pointer-heavy graphs where it helps.
- Bounded parallelism (worker count matched to available cores) with deterministic merging.
- Stream large outputs. Never load more than necessary into memory at once.
- Assembly or platform-specific code only after profiling proves a need (section 2.5).

### 4.3 Measurement

Benchmarks run on synthetic projects of fixed sizes (generated deterministically) and on the fixture corpus. CI records benchmark results as trends, since shared runners are noisy. Budgets are checked on reference hardware before releases. Profiles (CPU and memory) accompany performance-related pull requests (see [TESTING.md](TESTING.md)).

## 5. Portability

- **Targets:** Linux, Windows, and macOS on x86_64 and ARM64, with support tiers defined in [RELEASES.md](RELEASES.md).
- **Paths:** internally normalized, slash-separated, and relative to the root. Native host forms (drive letters, UNC paths, backslashes used as separators) stay at filesystem boundaries. Source names are preserved for analysis; portable output names have a separate stronger validation contract (`D-053`, not implemented yet). Long paths on Windows must be tested.
- **Case sensitivity:** never rely on case-insensitive lookups. Output path collisions that differ only in case are detected (`FS4002`).
- **Unicode normalization:** paths are compared byte-wise. Names that differ only in Unicode normalization (NFC versus NFD) are detected and reported.
- **Invalid output names:** Windows reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`), trailing dots or spaces, and control characters are rejected for output paths (`FS7003`).
- **Line endings and byte order marks:** preserved in transformed files. New files follow the project's dominant line ending, never the operating system's default.
- **File modes:** executable bits are preserved for verbatim copies on POSIX systems. Ownership is never changed.
- **Symbolic links on Windows:** creating them may require privileges, so tests that need them skip themselves when unavailable.
- **Locale and time zone:** never affect output. String ordering is byte-wise.

## 6. Determinism rules for code

1. Never let output depend on hash map iteration order. Sort keys before serializing or iterating for output.
2. Never let output depend on filesystem enumeration order. Sort directory entries.
3. Never use wall-clock time, random numbers, process IDs, or host names in engine results. Timestamps appear only in the report's `run` section.
4. Store parallel results by precomputed index, never by completion order.
5. Sort with a total order. Ties are broken by stable keys (path, then offset, then ID).
6. Compare strings byte-wise, never with locale-aware collation.
7. Use integer arithmetic for metrics (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)).
8. Never let environment variables influence results. Every behavior-relevant input is an explicit option recorded in the plan.
9. Derive identifiers from content or stable paths, never from shared counters across concurrent workers.
10. Use explicit, recorded versions for every embedded data set (knowledge base, target profiles, weights).

## 7. Code quality and structure

### 7.1 Boundaries

The existing M0 crates follow the layering in section 6 of [ARCHITECTURE.md](ARCHITECTURE.md). The M1 CI script `.github/scripts/check.ps1` enforces explicit Cargo crate dependencies, including dev, build, and target-specific dependencies (`D-051`). Source-level framework import checks remain later adapter work. Later implementation must preserve downward-only dependencies. The CLI depends only on the engine's public interface. Packages are small and cohesive. "utils" or "common" packages are not allowed to become dumping grounds.

### 7.2 Error handling

- Expected conditions in the source project (unsupported syntax, unknown packages) become findings, not errors.
- Operational errors (I/O failures, missing permissions) are returned with context and mapped to exit codes by the CLI.
- Internal invariant violations become internal errors (`FS9001`, exit code 70), and the run is marked untrustworthy.
- Fallible operations return results. No panics cross the engine API. Catching a panic is allowed only around isolated per-file tasks, which then report `FS9001`. It is never used to continue silently. Release builds unwind on panic, so this isolation works.

### 7.3 Fail-closed code

- Matches over closed kinds enumerate every variant explicitly. Unexpected values at open input boundaries report an internal error or classify the input as `unknown`; they never produce silent success.
- Exhaustiveness is enforced by the compiler: matches over closed enumerations in core code have no wildcard arms, so a new variant breaks the build until every match handles it (section 2.5).
- A helper whose fallback rejects its input, such as the parser's reinterpretation of cover expressions as patterns, may test only the kinds it accepts with `if let` or `matches!`. A new kind then fails closed rather than succeeding silently. Any match whose fallback could succeed enumerates every variant.
- Rules return explicit results: an operation, a classification with a reason, or "not applicable". There is no implicit success.

### 7.4 Concurrency

Bounded worker pools, cooperative cancellation, no shared mutable state between workers, and no leaked background work. Rust's type system rules out data races in safe code. Code under `D-019` that crosses a language boundary is additionally tested with sanitizers.

### 7.5 Comments and documentation

- Comments explain why, not what. Doc comments describe the contract of exported identifiers.
- Rules link to the section of [MIGRATION_SPEC.md](MIGRATION_SPEC.md) they implement.
- Every finding code has documentation that `frameshift explain` can print.
- Comment density follows the surrounding code. No boilerplate comments.

### 7.6 Review checklist

- Does the change keep results deterministic (section 6)?
- Do unexpected inputs fail closed?
- Are paths validated, and is execution of external code authorized (see [SECURITY.md](SECURITY.md))?
- Are tests proportionate, and do they cover the error boundaries (see [TESTING.md](TESTING.md))?
- Are documentation and `AGENTS.md` updated if behavior, commands, or conventions changed?
- Does the change add a dependency, and if so, is there a decision record?

## 8. Naming conventions

| Item | Convention |
| --- | --- |
| Product name | Frameshift, with exactly this capitalization. Never "FrameShift", "Frame Shift", or "frame-shift". |
| Executable and command | `frameshift` (`frameshift.exe` on Windows) |
| Repository | `eiserv/frameshift`, private (`D-026`) |
| Rust crates | A Cargo workspace. Crates are named `frameshift-<subsystem>` (for example `frameshift-engine`, `frameshift-cli`). The binary is `frameshift`. Development-only tools live under `tools/`, are named `frameshift-<purpose>`, and are never dependencies of product crates. Modules inside crates use short, lowercase names. The existing M0 crate layout is recorded in [IMPLEMENTATION.md](IMPLEMENTATION.md); it expands as concrete M1 requirements arise. |
| JSON document kinds | `frameshift.version` and usage `frameshift.error` implemented (`D-050`); `frameshift.analysis`, `frameshift.plan`, `frameshift.report`, and `frameshift.manifest` planned |
| Output metadata | `.frameshift/` directory (with `SUMMARY.md`, `manifest.json`, `report.json`, `unmigrated/`), `src/frameshift-compat/` helpers |
| Configuration file | `frameshift.jsonc` (`D-027`) |
| Environment variable prefix | `FRAMESHIFT_` (no public runtime options defined). `FRAMESHIFT_RUSTC_VERSION` is private Cargo-injected compiler metadata (`D-050`). `FRAMESHIFT_BUILD_COMMIT` is an optional validated build input (`D-051`). Neither is read at runtime. Repository variables prefixed `FRAMESHIFT_` control release automation only. |
| Finding codes | `FS` followed by four digits, by area (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)) |
| Source adapter IDs | Lowercase framework name, for example `nextjs` |
| Target IDs | Lowercase, hyphen-separated, for example `astro-preact` |
| Migration path IDs | `<source>-to-<target>`, for example `nextjs-to-astro-preact` |
| Rule IDs | `<migration-path-id>/<area>.<name>`, for example `nextjs-to-astro-preact/route.static-page` |
| Feature IDs | `<ecosystem>.<category>.<name>`, for example `nextjs.api.link` |
| Unit IDs | `<kind>:<path or URL pattern>[#<export>]`, for example `component:components/Hero.tsx#Hero` |
| Target profile IDs | Framework and major versions, for example `astro7-preact10` |
| Versions of embedded data | Knowledge base `YYYY.MM.N`, weights `weights-vN` |
| Release artifacts | See [RELEASES.md](RELEASES.md) |

Conventional Commit scopes follow the subsystems. Recommended scopes: `cli`, `core`, `discovery`, `parser`, `analyzer`, `graph`, `model`, `rules`, `planner`, `transform`, `emit`, `verify`, `report`, `kb`, `security`, `migration`, `deps`, `ci`, `release`, plus document names for documentation (for example `docs(architecture)`).

## 9. Development practices for future work

- **Specification before rules.** A rule starts with its entry in [MIGRATION_SPEC.md](MIGRATION_SPEC.md) and its fixtures. Code follows.
- **One rule, one pull request.** A pull request adding a rule includes the rule, positive and negative fixtures, golden outputs, `explain` documentation, and a compatibility matrix update.
- **Decision records.** Language, dependencies, document schema changes, and weight changes are recorded in [DECISIONS.md](DECISIONS.md).
- **Small, complete increments.** Prefer shipping a narrow, finished capability over a broad, half-finished one. Unfinished features stay out of releases.
- **Documentation synchronization.** Every change that alters behavior, commands, conventions, or limits updates the affected documents in the same pull request.

## 10. Open questions

- Concrete parser recovery synchronization points and default diagnostic/recovery budgets (M3, under accepted `D-034`).
- HTML parser for static output comparison: build or adopt (`Q-005`).
