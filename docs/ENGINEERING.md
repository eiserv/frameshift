# Engineering

**Status: guidance and proposals.** No toolchain is initialized and no code exists. This document evaluates implementation languages, defines the build-versus-adopt policy for dependencies, sets performance goals, and establishes engineering conventions for future work. Recommendations are recorded as "Proposed" in [DECISIONS.md](DECISIONS.md) until accepted. Go is an accepted *eligible* language (`D-015`). It is not a selected language.

## 1. Engineering principles

- **Correctness and safety before speed of delivery.** There are no deadlines. Work that improves long-term correctness is worth its cost.
- **KISS and YAGNI.** Solve the current stage's concrete requirements with the simplest structure. No speculative abstractions, configuration options, or extension points.
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

### 2.3 Go, evaluated in detail

Go is treated as a first-class candidate. A garbage collector and a higher-level development experience are not disqualifying for this workload.

| Question | Assessment |
| --- | --- |
| Fast development of reliable cross-platform CLIs | Strong. Small language, consistent formatting (`gofmt`), fast compilation, a standard library that covers file handling, processes, JSON, and text. The standard `flag` package is basic, but subcommand dispatch is easy to build on top of it. |
| Cross-compilation | Strong. With CGO disabled, one toolchain on one host produces binaries for every planned target by setting `GOOS` and `GOARCH`, including `windows/arm64` and both macOS architectures. |
| Self-contained binaries | Strong. With CGO disabled, Linux binaries are fully static. On macOS and Windows, Go links only against system libraries, as the platforms require. Binaries are larger than equivalent Rust binaries, but stripping symbols keeps them moderate. |
| Goroutines and concurrency | Strong fit for parallel per-file parsing: bounded worker pools, channels, and cancellation through contexts. Determinism requires storing results by index rather than by completion order, as in any language. |
| Runtime performance and memory | Good. Compiled native code. Typically more memory than Rust because of collector headroom. Tunable with `GOGC` and `GOMEMLIMIT`, and reducible by allocation discipline (slices of structs, indices instead of pointers, string interning). |
| Garbage collection overhead compared with Rust, Zig, C, and C++ | Real but acceptable. Those languages give deterministic memory use and lower peak memory. For a batch tool whose end-to-end time is dominated by external builds, collector overhead is a small fraction of total time. Pause latency is irrelevant. |
| Parsing, transformation, compiler-like workloads | Proven by the TypeScript native compiler and esbuild. The main weakness is the lack of sum types and exhaustive pattern matching, which makes tree code more verbose and lets a missing case compile. Mitigations are in section 7.3 and `Q-010`. |
| JavaScript, TypeScript, JSX, and TSX parsing infrastructure | Weak as reusable libraries. No first-party public Go parser exists: esbuild's and the TypeScript compiler's parsers are internal packages. Community forks expose them, but forking means owning code designed for other purposes. tree-sitter bindings require CGO. In practice, Go means building Frameshift's own parser (`P-002`), which matches the dependency philosophy but is the largest single work item. |
| Foreign-function interoperability | Adequate. cgo works, and Go 1.26 reduced its call overhead by about 30 percent. It still adds a C toolchain to the build. |
| CGO implications | Significant. CGO requires a C toolchain for every target, breaks single-host cross-compilation (unless a cross C compiler such as `zig cc` is configured), complicates fully static Linux builds, and complicates reproducibility. Proposal: CGO disabled as a hard rule. Any need for foreign code is handled across a process boundary instead. |
| Long-term maintainability and toolchain simplicity | Strong. The Go 1 compatibility promise keeps old code compiling. One toolchain provides building, testing, benchmarking, fuzzing, race detection, profiling, and vulnerability scanning (`govulncheck`). The language is easy for reviewers to read. |

### 2.4 Rust, evaluated in detail

- **Strengths:** top-tier performance and memory efficiency without a garbage collector. Memory safety with data-race freedom. Algebraic data types with exhaustive pattern matching, which suit syntax trees, IR variants, and rule results especially well. A strong WebAssembly story. The richest ecosystem of JavaScript tooling components (oxc, swc, Biome, Lightning CSS).
- **Weaknesses for Frameshift:** the standard library lacks JSON, argument parsing, and benchmarking and fuzzing harnesses. Under a strict dependency policy, Frameshift would have to build more basics itself, or adopt more crates. Cross-compiling, especially to macOS, needs target-specific linkers and SDKs, so native CI runners per platform are the practical route. Compile times are longer. Ownership rules add friction to graph-shaped data, usually solved with arenas and indices. The learning curve is steeper for contributors.
- **Adoption option:** with oxc behind a Frameshift-owned facade, Rust would reach a working parser fastest. The cost is a large, fast-moving dependency in the most critical component, against the build-versus-adopt criterion "control over critical behavior" (section 3.2).

### 2.5 Other candidates

- **Zig.** Excellent control, performance, and cross-compilation. However, it is pre-1.0 with ongoing breaking changes, it is not memory-safe (safety checks catch some errors in checked build modes), and it has no reusable JS/TS parser. Too risky as a foundation for a long-lived project today. Possible future role: `zig cc` as a cross C toolchain, if a C dependency ever becomes unavoidable. That would be a build tool, not a language mix.
- **C.** Maximum portability and performance, but memory-unsafe parsing of untrusted input is a severe liability. tree-sitter provides concrete syntax trees, not the semantic analysis Frameshift needs. Rejected.
- **C++.** Same memory-safety concerns as C, plus complex build systems and toolchains. Rejected.
- **Assembly.** Only justified for a measured, specialized hotspot (for example, vectorized byte scanning in a lexer) after profiling shows that high-level code cannot meet a budget. Both Go and Rust allow targeted assembly or intrinsics if that ever happens. No such need is expected.
- **Haskell.** Strong for language processing: algebraic data types, parser combinators, pure transformations. The costs are garbage collection with laziness-related space leaks, friction with static distribution (especially on macOS), weak JS/TS tooling, a smaller contributor pool, and interoperability costs if combined with another language. Not selected. It could serve for offline research, for example prototyping the semantics of a future recipe language, but would not be shipped.
- **OCaml.** A credible compiler language (algebraic data types, fast native code, a garbage collector, a long track record in language tooling). Weaker Windows support and cross-compilation history, and a smaller ecosystem. Not selected. It would be reconsidered only if both Go and Rust were ruled out.
- **TypeScript on Node.js or Bun.** The best semantic tooling for the source language (the TypeScript compiler API, Babel). However, it requires a JavaScript runtime or large bundled executables, its dependency culture conflicts with the dependency policy, and it is slower. The TypeScript team itself moved its compiler to Go for performance. Rejected for the engine.

### 2.6 One language or several

| Combination | Assessment |
| --- | --- |
| Go engine with a Rust parser through cgo | Needs Rust and C toolchains for every target, loses CGO-free cross-compilation, and complicates reproducible builds. If a Rust parser were adopted, an all-Rust implementation would be simpler. Rejected. |
| Go engine with a Rust parser compiled to WebAssembly and run inside Go | Portable, but adds a WebAssembly runtime dependency and call overhead to the most performance-sensitive path. Rejected. |
| Go or Rust engine with optional external helper processes | Acceptable later for isolated, opt-in capabilities, such as invoking a type checker as an external tool in a later stage. The process boundary keeps the core simple and portable. |
| Rust with Zig | No benefit beyond what Rust provides. Rejected. |
| Rust with C (tree-sitter) for multi-language parsing in V3 | Possible later. Out of scope now. |
| Any core language with Haskell for a rule language | Interoperability cost too high. Rejected. |

Conclusion: a **single implementation language** for engine and CLI. A second language requires an accepted decision record that shows a measured benefit outweighing the integration cost.

### 2.7 Recommendation

**Recommended direction (`P-001`, Proposed): Go as the single language for engine and CLI, with CGO disabled, standard library first, and purpose-built parsers (`P-002`).**

Reasoning:

1. **The dependency philosophy neutralizes Rust's largest advantage.** Rust's lead is its reusable JavaScript tooling ecosystem. Frameshift's policy is to build components whose behavior is critical to correctness, and parsing is the most critical one. Once Frameshift builds its own parsers, the comparison is between the languages themselves.
2. **Go's standard library covers almost everything else Frameshift needs.** JSON, hashing, filesystem traversal, concurrency, process control, archives, testing, benchmarks, fuzzing, profiling, and race detection all come without third-party code. A Rust implementation with equally few dependencies would need to build more of these basics.
3. **Distribution and reproducibility are simpler.** CGO-free Go builds every target from one host, with one pinned toolchain.
4. **Maintainability over a long horizon.** The Go 1 compatibility promise, fast builds, and readable code suit a long-lived project reviewed by humans and agents.
5. **Performance is sufficient by evidence.** Go powers the TypeScript native compiler and esbuild, and Frameshift's own work is small compared with external verification.

Accepted costs and mitigations:

- *No sum types:* closed kind enumerations, fail-closed default branches, and an exhaustiveness check for switches over closed kinds (built as a small analysis tool, or adopted after review: `Q-010`).
- *Building a parser:* a large up-front investment, bounded by specifications and conformance suites, and validated early by experiment E1.
- *Higher memory than Rust:* compact representations and allocation discipline, and budgets checked in benchmarks.

**Main alternative:** Rust as the single language, with purpose-built parsers or with oxc behind a facade. Rust becomes the better choice if any condition in section 2.9 occurs.

### 2.8 Experiments that decide the language

These experiments must finish before milestone M1 (see [ROADMAP.md](ROADMAP.md)), because switching languages later becomes much more expensive.

**E1: parser feasibility in Go.**

- Scope: a lexer and parser for JavaScript, TypeScript, JSX, and TSX that covers modules, statements, expressions (including arrow functions, async and await, optional chaining, template literals, destructuring), classes, JSX and TSX (including the generic arrow ambiguity), and TypeScript type syntax (parsed and preserved as spans). The AST representation must be designed for the real implementation, not as a throwaway.
- Corpus: Frameshift's fictional fixtures plus a sample of real-world Next.js App Router files. Real-world files are kept outside the repository, and their licenses are respected.
- Success criteria:
  - At least 99 percent of corpus files parse.
  - Every failure is explained by syntax not yet implemented, never by a design flaw.
  - Span correctness is checked by slicing tests.
  - Throughput of at least 20 MB/s per core.
  - Peak memory of at most 30 times the source size with full tree retention.
  - Fuzzing for a fixed CPU budget finds no crashes or hangs.
  - A documented review concludes that the tree and visitor design is maintainable without sum types.
- Thresholds are set to expose design problems, not to win benchmarks.

**E2: edit-based rewriting and Astro printing.**

- Scope: the span edit engine and a minimal Astro component printer, applied end to end to three fixtures: a static page with links, a layout with metadata, and a client counter island.
- Success criteria:
  - Comments and formatting are preserved in untouched regions.
  - Output is byte-identical across repeated runs and across operating systems.
  - Provenance records map every output region back to its source.
  - Astro builds the output with the pinned target profile.
  - The result confirms or refutes the assumption that a span-precise AST is enough without a concrete syntax tree (`P-003`).

Later experiments, run before the milestones that depend on them, are listed in [DECISIONS.md](DECISIONS.md): E3 (static evaluation coverage), E4 (static output comparison), and E5 (draft and immutable release flow).

### 2.9 Conditions that would change the recommendation

- E1 misses the throughput or memory budget by more than a factor of two, and design changes cannot close the gap.
- The E1 review judges the tree design unmaintainable without sum types.
- The project decides to adopt an existing parser instead of building one. Rust with oxc would then be the most coherent stack.
- WebAssembly embedding becomes a primary requirement, for example for a browser-based frontend.

### 2.10 Decisions that can safely be deferred

Plugin mechanism (`DF-002`), WebAssembly target (`DF-012`), GUI technology (`DF-006`), any foreign-function interface, model-assisted processing (`DF-005`), and code signing (`DF-004`) do not affect the language choice now. The engine API and JSON contracts keep later options open.

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
9. **Portability:** no CGO, works on every target platform.
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

Valid under the Go recommendation. They will be re-evaluated if the language decision changes.

| Component | Category | Proposal | Notes |
| --- | --- | --- | --- |
| JS, TS, JSX, TSX parser | Runtime | Build (E1) | Most critical component. Needs exact spans, comments, error policy, and resource limits. |
| CSS tokenizer and structural parser | Runtime | Build | Specified by CSS Syntax Level 3. Bounded. |
| JSON | Runtime | Standard library for reading. A small deterministic writer where key order or formatting must be controlled. | |
| JSONC | Runtime | Build | Small |
| YAML subset for lockfiles | Runtime | Build | General YAML is large and risky. Only the lockfile subset is needed. |
| Yarn classic lockfile | Runtime | Build | Small format |
| npm semver range matching | Runtime | Build | Needed for knowledge base matching. The grammar is well documented. |
| gitignore and glob matching | Runtime | Build | Specified behavior, small |
| SHA-256 hashing | Runtime | Standard library | Never build cryptography |
| Argument parsing | Runtime | Build on the standard library | |
| Terminal detection and color | Runtime | Build minimally, or evaluate `golang.org/x/term` | Modules under `golang.org/x/` are maintained by the Go project but are outside the Go 1 compatibility promise, so they count as external dependencies |
| Text diff for previews | Runtime | Build if needed | Myers' algorithm |
| HTML parsing for static output comparison | Runtime (verification) | Evaluate `golang.org/x/net/html` against building (E4, `Q-005`) | |
| Testing, benchmarking, fuzzing, profiling | Development | Standard toolchain | |
| Additional linters | Development | Evaluated individually | Each one needs a reason |
| Vulnerability scanning | Development | `govulncheck` | |
| Release automation | CI | Release Please action, GitHub CLI | Pinned (see [RELEASES.md](RELEASES.md)) |
| Headless browser for validating Frameshift's own fixtures | CI only | External tool, never shipped. Decided in M10. | |

### 3.6 Process

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
| Parser throughput (E1) | at least 20 MB/s per core | at most 30 times the source size with full retention |

### 4.2 Practices

- Algorithms over files, nodes, and graph edges must be linear or near-linear. Quadratic behavior requires a written justification.
- Parse each file once. Share immutable results between stages.
- Intern identifiers and paths. Use compact node representations and indices instead of pointer-heavy graphs where it helps.
- Bounded parallelism (worker count matched to available cores) with deterministic merging.
- Stream large outputs. Never load more than necessary into memory at once.
- Assembly or platform-specific code only after profiling proves a need (section 2.5).

### 4.3 Measurement

Benchmarks run on synthetic projects of fixed sizes (generated deterministically) and on the fixture corpus. CI records benchmark results as trends, since shared runners are noisy. Budgets are checked on reference hardware before releases. Profiles (CPU and memory) accompany performance-related pull requests (see [TESTING.md](TESTING.md)).

## 5. Portability

- **Targets:** Linux, Windows, and macOS on x86_64 and ARM64, with support tiers defined in [RELEASES.md](RELEASES.md).
- **Paths:** internally normalized, slash-separated, and relative to the root. Platform-specific forms (drive letters, UNC paths, backslashes) exist only at the CLI boundary. Long paths on Windows must be tested.
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

The layering in section 6 of [ARCHITECTURE.md](ARCHITECTURE.md) is enforced by the package structure and by an automated import-boundary check. The CLI depends only on the engine's public interface. Packages are small and cohesive. "utils" or "common" packages are not allowed to become dumping grounds.

### 7.2 Error handling

- Expected conditions in the source project (unsupported syntax, unknown packages) become findings, not errors.
- Operational errors (I/O failures, missing permissions) are returned with context and mapped to exit codes by the CLI.
- Internal invariant violations become internal errors (`FS9001`, exit code 70), and the run is marked untrustworthy.
- No panics cross the engine API. Recovering from a panic is allowed only around isolated per-file tasks, which then report `FS9001`. It is never used to continue silently.

### 7.3 Fail-closed code

- Every switch over a closed set of kinds (syntax node kinds, unit kinds, feature IDs, statuses) has an explicit default branch that reports an internal error or classifies the input as `unknown`. A silent default is never allowed.
- Exhaustiveness of such switches is checked automatically. The mechanism depends on the language decision (`Q-010`).
- Rules return explicit results: an operation, a classification with a reason, or "not applicable". There is no implicit success.

### 7.4 Concurrency

Bounded worker pools, cooperative cancellation, no shared mutable state between workers, and no leaked background work. The race detector runs in CI where the toolchain provides one.

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
| Repository name | `frameshift` |
| Go module path (if Go is accepted) | `<host>/<owner>/frameshift`, fixed when the repository location is known. Package names are short, lowercase, and do not repeat the product name. For example, a planner package is named `plan`, not `frameshiftplan`. |
| Rust crates (if Rust is chosen) | `frameshift-<subsystem>`, binary `frameshift` |
| JSON document kinds | `frameshift.analysis`, `frameshift.plan`, `frameshift.report`, `frameshift.manifest`, `frameshift.error` |
| Output metadata | `.frameshift/` directory, `FRAMESHIFT.md` summary, `src/frameshift-compat/` helpers |
| Environment variable prefix | `FRAMESHIFT_` (reserved, none defined) |
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

- Exhaustiveness checking for closed kinds in Go (`Q-010`).
- Parser error recovery depth (`Q-011`).
- HTML parser for static output comparison: build or adopt (`Q-005`).
