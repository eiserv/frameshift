# Decision Register

This register records Frameshift's architectural and project decisions. Only entries with the status **Accepted** are binding. **Proposed** entries are recommendations that still need confirmation, usually through an experiment or an explicit decision by the project owner. Entries keep their ID when their status changes, so some `P-` entries are now Accepted. **Deferred** entries are intentionally postponed. **Open questions** have no recommendation yet, or only a tentative one.

Statuses change through an edit to this file under the integration workflow in D-010, including any explicitly authorized exception. A changed decision keeps its ID. The entry records the new status, the date, and the reason, and older reasoning remains visible in the Git history.

## Index

| ID | Title | Status |
| --- | --- | --- |
| D-001 | Project name Frameshift and CLI command `frameshift` | Accepted |
| D-002 | English for all repository artifacts | Accepted |
| D-003 | Writing style rules | Accepted |
| D-004 | CLI-only product surface in the current scope | Accepted |
| D-005 | Engine independent of the CLI | Accepted |
| D-006 | Deterministic, offline, rule-based engine; AI optional and deferred | Accepted |
| D-007 | First migration path: Next.js App Router to Astro + Preact (static) | Accepted |
| D-008 | Source repositories are untrusted | Accepted |
| D-009 | Honest reporting and fail-closed classification | Accepted |
| D-010 | Version control workflow | Accepted |
| D-011 | Release management with Release Please | Accepted |
| D-012 | Private repository phase | Accepted |
| D-013 | Minimal external dependencies with build-versus-adopt criteria | Accepted |
| D-014 | Capability-based milestones without deadlines | Accepted |
| D-015 | Go is an eligible implementation language | Superseded by D-018 |
| D-016 | Three-stage development strategy | Accepted |
| D-017 | Documentation structure and canonical agent instructions | Accepted |
| D-018 | Rust as the primary implementation language | Accepted |
| D-019 | Additional languages for specific, important tasks | Accepted |
| D-020 | Quality over effort | Accepted |
| D-021 | Naming of public environment variables | Accepted |
| D-022 | Monorepo scope in V1 | Accepted |
| D-023 | Supported Next.js versions in V1 | Accepted |
| D-024 | Conventional Astro output layout | Accepted |
| D-025 | Splitting files with several components | Accepted |
| D-026 | Repository location | Accepted |
| D-027 | Configuration file `frameshift.jsonc` in V1 | Accepted |
| D-028 | Target profile policy | Accepted |
| D-029 | Location format | Accepted |
| D-030 | Selection of the real-world evaluation corpus | Accepted |
| D-031 | Second migration path | Accepted |
| D-032 | M0 tooling and first implementation increment | Accepted |
| D-033 | Mandatory forward-looking architecture | Accepted |
| D-034 | Bounded parser error recovery | Accepted |
| D-035 | Template lexical components as the next M0 increment | Accepted |
| D-036 | Pinned Unicode identifiers and regular-expression literals as the next M0 increments | Accepted |
| D-037 | Complete regular-expression validation with Annex B, then the syntax tree arena | Accepted |
| D-038 | Expression grammar completion with in-place pattern reinterpretation | Accepted |
| D-039 | Complete JavaScript grammar with explicit source goals and full sloppy mode | Accepted |
| D-040 | Built measurement harness with a counting allocator, then a throughput increment before JSX | Accepted |
| P-001 | Go as the single implementation language, CGO disabled | Superseded by D-018 |
| P-002 | Purpose-built parsers | Proposed |
| P-003 | Span-based edits and targeted printers; no concrete syntax tree initially | Proposed |
| P-004 | MVP project model instead of a universal IR | Accepted |
| P-005 | Phase separation and recomputed plans | Accepted |
| P-006 | Fixed kind-based weights (`weights-v1`) | Accepted |
| P-007 | Conversion ladder for components | Accepted |
| P-008 | `preact/compat` as the default island strategy | Accepted |
| P-009 | `client:load` as the default hydration directive | Accepted |
| P-010 | Lockfile carry-over instead of lockfile generation | Accepted |
| P-011 | Output metadata and support file locations | Accepted |
| P-012 | Exit code scheme | Accepted |
| P-013 | Verification levels | Accepted |
| P-014 | Release workflow structure | Accepted |
| P-015 | Artifact naming and platform tiers | Accepted |
| P-016 | Finding codes and identifier schemes | Accepted |
| P-017 | Minimal relocation policy for output layout | Superseded by D-024 |
| DF-001 to DF-012 | Deferred decisions | Deferred or resolved (see table) |
| Q-001 to Q-012 | Open questions | Open or resolved (see table) |
| E1 to E5 | Experiments | E1 in progress; others planned |

## Accepted decisions

### D-001: Project name Frameshift and CLI command `frameshift`

- **Status:** Accepted (2026-10-11).
- **Decision:** The official project name is Frameshift, with exactly this capitalization. The executable and command name is `frameshift`. No alternative names, temporary branding, or unrelated package identities are used. Internal naming follows the conventions in section 8 of [ENGINEERING.md](ENGINEERING.md).
- **Rationale:** One consistent identity across documentation, code, artifacts, and releases.

### D-002: English for all repository artifacts

- **Status:** Accepted (2026-10-11).
- **Decision:** English is the working language for all documentation, source code and comments, issues, pull requests, reviews, commit messages, changelogs, release notes, error messages, CLI output, configuration descriptions, and decision records. Future UI text is also English unless internationalization is introduced deliberately. Direct chat communication with the user is the only exception, and uses the user's language.
- **Rationale:** One shared language keeps the project accessible and consistent, and prepares it for a possible public release.

### D-003: Writing style rules

- **Status:** Accepted (2026-10-11).
- **Decision:** Clear, precise, professional English. No em dashes in the repository. Use commas, colons, semicolons, parentheses, or separate sentences instead. No marketing language or vague claims. Explain important decisions and tradeoffs. Distinguish confirmed facts from proposals, and label unresolved decisions.

### D-004: CLI-only product surface in the current scope

- **Status:** Accepted (2026-10-11).
- **Decision:** Frameshift is distributed and operated as a command-line application. No GUI, web interface, desktop application, or frontend is part of the current scope. The CLI is meant to become a production-quality tool, not a prototype.
- **Tradeoff:** Users who prefer graphical tools are not served for now. The engine's independence (D-005) keeps a later interface possible.

### D-005: Engine independent of the CLI

- **Status:** Accepted (2026-10-11).
- **Decision:** All migration logic lives in the engine. The CLI is a thin adapter. Future frontends reuse the engine through its API or through versioned JSON documents, never through a second implementation.

### D-006: Deterministic, offline, rule-based engine; AI optional and deferred

- **Status:** Accepted (2026-10-11).
- **Decision:** The engine is deterministic, algorithmic, and rule-based. It works fully offline, and its normal operation never depends on an LLM or external AI service. Model-assisted capabilities are deferred and, if ever added, remain optional and subordinate to deterministic rules, as defined in [LLM_POLICY.md](LLM_POLICY.md).

### D-007: First migration path

- **Status:** Accepted (2026-10-11).
- **Decision:** The first migration path is Next.js (App Router) to Astro with Preact islands, targeting static output (`nextjs-to-astro-preact`). The main target is mostly static sites that use Next.js unnecessarily. The goals are less client-side JavaScript, preserved interactivity, a simpler architecture, and preserved routes, content, appearance, accessibility, and behavior. The specification is [MIGRATION_SPEC.md](MIGRATION_SPEC.md).

### D-008: Source repositories are untrusted

- **Status:** Accepted (2026-10-11).
- **Decision:** Source projects are treated as potentially hostile. Frameshift never modifies them, never executes their code without explicit per-invocation authorization, and never writes outside the designated output location. See [SECURITY.md](SECURITY.md).

### D-009: Honest reporting and fail-closed classification

- **Status:** Accepted (2026-10-11).
- **Decision:** Frameshift never presents an incomplete, unverified, or unknown result as successful. Anything not positively handled by a rule is reported as unknown or unsupported. Percentages are always defined, and they are never presented as proof of behavioral equivalence. See [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).

### D-010: Version control workflow

- **Status:** Accepted (2026-10-11).
- **Decision:** Short-lived feature branches. Small, self-contained commits. Conventional Commits in English for commit messages and pull request titles. Pull-request-based integration into `main`. Squash merges only. Unrelated changes in separate branches and pull requests. See [RELEASES.md](RELEASES.md).
- **Repository configuration (2026-10-12):** At the project owner's request, enabled only squash merging on `eiserv/frameshift`, disabled merge commits and rebase merging, and selected the PR title and body as the squash commit defaults. Read-back confirmed these settings and private visibility. Branch protection remains a separate setup item.
- **One-run exception (2026-10-12):** The project owner explicitly authorized committing the verified template-scanner increment directly on `main`, then explicitly authorized pushing it to `origin/main` in a follow-up instruction. This overrides the pull-request and direct-push restrictions for this increment and its documentation updates only. It does not change the default workflow or establish branch protection.
- **Second one-run exception (2026-10-12):** Through the Claude Code chat, the project owner explicitly authorized committing and pushing directly to `main` for the `D-036` increments and their documentation, once the agent considers the work complete. This exception has the same limits as the first one.
- **Third one-run exception (2026-10-12):** Through the Claude Code chat, the project owner explicitly authorized committing and pushing directly to `main` for the `D-037` regular-expression increment and its documentation, once the agent is satisfied with the work. This exception has the same limits as the first one.
- **Fourth one-run exception (2026-10-12):** The project owner explicitly authorized committing and pushing directly to `main` for the bounded expression-parser increment and its documentation, if the agent is satisfied with the work. This exception has the same limits as the first one.
- **Fifth one-run exception (2026-10-12):** Through the Claude Code chat, the project owner explicitly authorized committing and pushing directly to `main` for the `D-038` expression-completion increment and its documentation, once the agent is satisfied with the work. This exception has the same limits as the first one.
- **Sixth one-run exception (2026-10-12):** Through the Claude Code chat, the project owner explicitly authorized committing and pushing directly to `main` for the `D-039` complete-grammar increment and its documentation, once the agent is satisfied with the work. This exception has the same limits as the first one.
- **Seventh one-run exception (2026-10-12):** Through the Claude Code chat, the project owner explicitly authorized committing and pushing directly to `main` for the `D-040` measurement-harness and throughput increments and their documentation, once the agent is satisfied with the work. This exception has the same limits as the first one.
- **Eighth one-run exception (2026-10-12):** In a separate, later run, the project owner again explicitly authorized through the Claude Code chat committing and pushing directly to `main` once the agent is satisfied with the work. The run implements the `D-040` throughput increment and its documentation. This exception has the same limits as the first one.

### D-011: Release management with Release Please

- **Status:** Accepted (2026-10-11).
- **Decision:** Release Please derives versions and changelogs from Conventional Commits and maintains a release pull request. Merging it creates the tag and the GitHub Release. Packaging attaches binaries to that release. Version numbers are never edited by hand. The detailed workflow structure is accepted in P-014.

### D-012: Private repository phase

- **Status:** Accepted (2026-10-11).
- **Decision:** The repository stays private. No publication, visibility changes, public mirrors, uploads of project contents to external services, or telemetry without explicit approval. No license is assumed until one is chosen (DF-003). An open-source preparation checklist is in section 11 of [SECURITY.md](SECURITY.md).

### D-013: Minimal external dependencies with build-versus-adopt criteria

- **Status:** Accepted (2026-10-11).
- **Decision:** External libraries are adopted only when they are demonstrably suitable under the criteria in section 3 of [ENGINEERING.md](ENGINEERING.md). Otherwise, purpose-built internal implementations are preferred, with understandable structures and documented architectural rationale (`D-033`). Runtime dependencies need individual decision records.
- **Tradeoff:** More internal engineering effort in exchange for control, a smaller attack surface, and freedom from external constraints.

### D-014: Capability-based milestones without deadlines

- **Status:** Accepted (2026-10-11).
- **Decision:** The roadmap uses capability-based milestones ordered by technical dependencies, not calendar dates. See [ROADMAP.md](ROADMAP.md).

### D-015: Go is an eligible implementation language

- **Status:** Superseded by D-018 (2026-10-11).
- **Decision:** Go was a first-class candidate for the implementation, evaluated on equal footing with Rust, Zig, C, C++, Haskell, and others. The evaluation is kept in section 2.4 of [ENGINEERING.md](ENGINEERING.md). Go remains the documented alternative.

### D-016: Three-stage development strategy

- **Status:** Accepted (2026-10-11).
- **Decision:** Development proceeds in three stages: V1 (Next.js to Astro migration MVP), V2 (extensible migration engine), and V3 (universal migration platform). Stage contents are planned in [ROADMAP.md](ROADMAP.md) and refined over time.

### D-017: Documentation structure and canonical agent instructions

- **Status:** Accepted (2026-10-11).
- **Decision:** Each topic has one authoritative document under `docs/`, and the other documents link to it instead of repeating it. `AGENTS.md` is the canonical instruction document for all agents. `CLAUDE.md` only points to it. Documentation must always distinguish planned from implemented behavior.

### D-018: Rust as the primary implementation language

- **Status:** Accepted (2026-10-11). Supersedes D-015 and P-001.
- **Context:** The first evaluation recommended Go (P-001), weighted toward V1. The project owner asked for an unbiased re-evaluation against the long-term scope.
- **Decision:** Rust is the primary implementation language for engine and CLI, for all stages. No later change of language or rewrite is planned.
- **Rationale:**
  - Frameshift is a multi-language program transformation system, where exhaustive pattern matching over algebraic data types directly supports fail-closed correctness.
  - Multi-language transformation tools are predominantly built in Rust or OCaml, and the parsing ecosystem (tree-sitter, oxc, Lightning CSS) is strongest in Rust.
  - No garbage collector, memory safety, strong C ABI and WebAssembly support.
  - Full evaluation: section 2 of [ENGINEERING.md](ENGINEERING.md).
- **Tradeoffs:** Standard library gaps (handled per component), native CI runners per platform, longer compile times, and arena-based designs for graph data.
- **Revisit if:** only under the conditions in section 2.9 of [ENGINEERING.md](ENGINEERING.md).

### D-019: Additional languages for specific, important tasks

- **Status:** Accepted (2026-10-11).
- **Decision:** Other programming languages may be used for specific, important tasks when they provide a demonstrable advantage in correctness, quality, safety, performance, or capability that Rust cannot provide equally well. Each use needs its own decision record and must meet the requirements in section 2.7 of [ENGINEERING.md](ENGINEERING.md): a clear boundary, safety, reproducible builds on all tier 1 platforms, determinism, the same engineering standard, and independence of the core.
- **Rationale:** The long-term architecture is not fully known. Excluding the best tool for a specific problem up front would trade quality for uniformity.
- **Tradeoff:** Each additional language adds toolchain and review surface, which is why the requirements are strict.

### D-020: Quality over effort

- **Status:** Accepted (2026-10-11).
- **Decision:** Effort, time investment, and other economic considerations are never, on their own, reasons to choose a lower-quality solution. Decisions are judged on correctness, safety, maintainability, performance, and professionalism. Structures must remain understandable, and architectural preparation must have a documented rationale (`D-033`).

### D-021: Naming of public environment variables

- **Status:** Accepted (2026-10-11). Resolves Q-003.
- **Decision:** By default, `NEXT_PUBLIC_X` becomes Astro's `PUBLIC_X` (`import.meta.env.PUBLIC_X`), and the report lists every variable that must be renamed in `.env` files and hosting settings. An option (`--env-names keep`, or `envNames` in the configuration file) keeps the original names by extending Vite's `envPrefix`. The option is validated against each target profile in M8. If a profile cannot support it, the option is rejected for that profile with a clear error.
- **Rationale:** The default produces an idiomatic Astro project. The option serves teams that cannot change deployment settings.

### D-022: Monorepo scope in V1

- **Status:** Accepted (2026-10-11). Resolves Q-008.
- **Decision:** V1 migrates one selected package of a workspace. Sibling workspace packages are treated as external dependencies with unknown compatibility. Migrating whole workspaces is left to later stages.

### D-023: Supported Next.js versions in V1

- **Status:** Accepted (2026-10-11). Resolves Q-012.
- **Decision:** V1 validates Next.js 14.x to 16.x with fixtures. Projects on other versions are analyzed with a warning, and version-dependent rules behave conservatively.

### D-024: Conventional Astro output layout

- **Status:** Accepted (2026-10-11). Supersedes P-017 and resolves Q-009.
- **Decision:** The output follows the conventional Astro structure. All code moves under `src/` (`pages`, `layouts`, `components`, `styles`, `lib`, `assets`, and other code directories under their original names). Non-code data directories read at build time stay at their path, so filesystem paths stay valid. `public/` and configuration files stay at the root. Details are in section 16 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md).
- **Rationale:** The result should read like a hand-written Astro project (`D-020`). The additional import rewriting is deterministic.

### D-025: Splitting files with several components

- **Status:** Accepted (2026-10-11). Resolves Q-004.
- **Decision:** In V1, every component that meets the Astro preconditions becomes its own `.astro` file, even if several components share a source file. Remaining components and non-component exports stay in the original module. Module-local helpers used by converted components move into a helper module. All imports are rewritten. Details are in section 9.3 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md).

### D-026: Repository location

- **Status:** Accepted (2026-10-11).
- **Decision:** The private GitHub repository is `eiserv/frameshift`, with the default branch `main`.

### D-027: Configuration file `frameshift.jsonc` in V1

- **Status:** Accepted (2026-10-11). Supersedes DF-001.
- **Decision:** V1 supports an optional project configuration file named `frameshift.jsonc` (JSON with comments), validated against a published JSON Schema. Command-line flags take precedence over the file. The file can never enable executing external code, change overwrite policies, or point outside the source root. See section 5.1 of [CLI.md](CLI.md).
- **Rationale:** It reuses the JSONC parser Frameshift needs anyway and is familiar in the JavaScript ecosystem.

### D-028: Target profile policy

- **Status:** Accepted (2026-10-11). Resolves Q-007.
- **Decision:** Each Frameshift release supports exactly one target profile per target, for the current Astro major version. A new Astro major is adopted only after full validation. Preact 11 is adopted only when it is stable, officially supported by `@astrojs/preact`, and passes every island fixture.

### D-029: Location format

- **Status:** Accepted (2026-10-11). Resolves Q-001.
- **Decision:** Byte offsets are the canonical location in all documents. Lines and columns are one-based, and columns count Unicode scalar values. Later editor integrations convert to UTF-16 code units where their protocols require it.

### D-030: Selection of the real-world evaluation corpus

- **Status:** Accepted (2026-10-11). Resolves the process part of Q-006.
- **Decision:** From M5 onward, agents propose public, static-leaning Next.js App Router projects with suitable licenses and a reason for each. The project owner approves the list. The projects are never committed to the repository.

### D-031: Second migration path

- **Status:** Accepted (2026-10-11). Partly resolves DF-011.
- **Decision:** The preferred second migration path for V2 is Next.js to Astro with React islands (milestone M18). The second source framework (M19) is still open.

### D-032: M0 tooling and first implementation increment

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** Begin with a minimal Cargo workspace for M0 and tested source positions and a deterministic span edit core. Pin Rust 1.98.0, including rustfmt and Clippy. Use only the Rust standard library initially. The workspace uses edition 2024, forbids unsafe code, and denies wildcard arms over closed enums through Clippy.
- **Milestone boundary:** M0 needs buildable tooling to run E1 and E2. This tooling is permitted before M1. CI, releases, the product CLI, and migration commands remain M1 or later work. Neither M0 nor M1 is complete after this increment.
- **Experiment gates:** P-002 and P-003 remain Proposed until their experiment criteria are met. The edit core alone does not validate AST-based rewriting or Astro output. E1 uses at least 30 MB/s per core and at most 20 times the source size with full tree retention throughout the documentation.
- **Tool evaluation:** Cargo, rustc, rustfmt, and Clippy come from the pinned Rust toolchain. They are maintained with Rust, support the planned platforms, require no runtime dependency, and provide standard build, formatting, and lint contracts. Exact pinning avoids tool drift. A deliberate build commit can replace the pin.
- **Versioning:** Unpublished M0 crates omit the optional Cargo package version, which defaults to 0.0.0. This is not a release. Release Please will establish the managed version in M1; versions must not subsequently be changed by hand.
- **Rationale:** Validate reusable, framework-independent primitives before adding parser or migration behavior. Keep the experiment gates honest and the first change independently reviewable.

### D-033: Mandatory forward-looking architecture

- **Status:** Accepted (2026-10-12), explicitly requested by the project owner through the Codex App question interface.
- **Decision:** Every implementation step must consider the documented V1 to V3 evolution, subsystem boundaries, extensibility, and compatibility. Architectural preparation is mandatory. Record the future requirements supported by a design, its tradeoffs, and unresolved assumptions. The previous restriction to current-stage needs is removed. Keep structures understandable (KISS) and do not describe future contracts as implemented behavior.
- **Boundaries:** Existing decisions about the V1 project model, deferred product surfaces, dependency evaluation, and E1/E2 experiment gates remain binding. Change those decisions explicitly when new evidence or requirements demand it.
- **Rationale:** Frameshift is a multi-phase transformation platform. Early design must account for its long-term scope without hiding uncertainty or making unverified correctness claims.

### D-034: Bounded parser error recovery

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface. Resolves the policy part of Q-011.
- **Decision:** The future parser should report multiple syntax errors in one pass using bounded recovery at safe grammar boundaries. Recovery must not guess repairs or make erroneous files eligible for transformation. Any syntax error makes the entire file `unknown`, even if part of its tree was recovered.
- **Safety contract:** Explicit per-file diagnostic and recovery-work limits, guaranteed forward progress, and no recovery across a boundary whose lexical context is unknown. If safe recovery is unavailable or a limit is reached, stop parsing that file and report why. Local scanners continue to return one typed error without partial results; they do not implement recovery.
- **Rationale:** Multiple diagnostics reduce repeated analysis runs while preserving fail-closed classification. Parser-owned synchronization respects JSX, template, and regular-expression contexts.
- **Remaining implementation choices:** Concrete synchronization points and default diagnostic/recovery budgets are chosen and tested with the grammar in M3. The parser returns one typed error and has no recovery mechanism.

### D-035: Template lexical components as the next M0 increment

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** Prioritize parser-directed ECMAScript template lexical components as the next E1 foundation increment. Full Unicode identifier support and regular-expression scanning remain subsequent work. Continue with the standard library only, preserving the E1/E2 gates.
- **Implemented contract:** `scan_js_template` requires explicit start/continuation and tagged/untagged context, returns delimiter-inclusive and content spans, and stops at an unescaped backtick or `${`. Invalid escapes fail in untagged mode; tagged mode records the first invalid escape and thus an undefined cooked value. It never decodes values or parses substitutions. Each segment has an explicit byte limit, default 1 MiB.
- **Architectural rationale (`D-033`):** Segment boundaries leave expression grammar, nested templates, and lexical-goal changes with the parser. The parser (`D-037` to `D-039`) selects these contexts for the JavaScript grammar. Exact source spelling supports provenance-preserving edits and future V1 to V3 static evaluation without imposing a value representation. Raw/cooked value decoding and grammar-aware recovery remain unimplemented.
- **Tradeoffs:** A local scan cannot validate a whole template or its expressions. Separate segment limits do not bound a whole template; the parser enforces aggregate limits and chooses contexts. Tagged invalid-escape evidence must not be confused with an error or a decoded string.

### D-036: Pinned Unicode identifiers and regular-expression literals as the next M0 increments

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Claude Code question interface.
- **Decision:** Implement two separate E1 foundation increments, in this order: ECMAScript identifier names with full Unicode classification, then regular-expression literal scanning. Continue with the standard library only, and preserve the E1/E2 gates.
- **Unicode data:** Pin Unicode 18.0.0, the latest version on 2026-10-12. A standard-library-only development tool generates sorted `ID_Start` and `ID_Continue` range tables from the Unicode Character Database file `DerivedCoreProperties.txt`. Only the generated tables are committed, together with the source version, file date, byte size, code point totals, and Unicode's copyright and terms notice. The source file is downloaded deliberately for each regeneration and is not committed. The file used on 2026-10-12 had 1,159,889 bytes and the SHA-256 hash `09c928886a178fcafd93c29e4bd59073a058e5a100b716d425cb563ab50f68c9`. Changing the Unicode version changes this decision.
- **Identifier scope:** `IdentifierName` and `PrivateIdentifier`, including Unicode escapes. Reserved-word classification covers ECMAScript's `ReservedWord` list and the strict-mode reserved words. It compares the StringValue, including escaped spellings, without allocation, and reports whether escapes occurred. Contextual keywords (`async`, `of`, `get`, and others) and TypeScript keywords remain direct parser comparisons of unescaped spellings.
- **Implemented identifier contract:** `scan_js_identifier` returns the kind, exact and name spans, `has_escape`, and the reserved word matching the decoded StringValue, or `None` for private names. Escapes must denote code points permitted at their position. Details are in section 4.6 of [IMPLEMENTATION.md](IMPLEMENTATION.md).
- **Regular-expression scope:** Superseded by `D-037`, which the project owner confirmed on 2026-10-12. The earlier agent default (lexical grammar and flags only, pattern validation later) no longer applies.
- **Architectural rationale (`D-033`):** One identifier classifier serves the numeric literal boundary, identifier names, regular-expression flags, future JSX names, and the planned check of generated identifiers in printers (section 14.3 of [ARCHITECTURE.md](ARCHITECTURE.md)). Pinned tables keep results deterministic and independent of the Rust toolchain's internal Unicode version. Reserved words are lexical facts. Whether a name is permitted depends on strict mode, the module goal, and async or generator context, which only the parser knows.
- **Tradeoffs:** ECMAScript requires the latest Unicode version, so every new Unicode release needs a deliberate regeneration to remain conformant. The tables add about 12 KB to the binary. The derived data carries Unicode's notice obligations into `THIRD_PARTY_NOTICES.md` before any distribution.
- **Revisit if:** a new Unicode version is published, or the parser needs further Unicode properties. In that case, extend the generator rather than adding a dependency.

### D-037: Complete regular-expression validation with Annex B, then the syntax tree arena

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Claude Code question interface. Replaces the regular-expression scope that `D-036` listed as an unconfirmed agent default.
- **Context:** `D-036` fixed the order of the identifier and regular-expression increments. The scope of the regular-expression increment, the treatment of web-compatible pattern syntax, and the increment after it were still open.
- **Decision:**
  - The regular-expression increment covers the complete validity check of a literal, not only its lexical extent: the lexical grammar of ECMAScript section 12.9.5, the flag rules of `IsValidRegularExpressionLiteral` (section 13.2.7.2), and the pattern grammar of section 22.2.1 with its early errors. This includes the `u` and `v` modes, named groups (including duplicate names in different alternatives), modifiers, and Unicode property escapes.
  - Without the `u` or `v` flag, the web-compatible pattern grammar of Annex B.1.2 applies, as in browsers and Node.js. With either flag, the strict grammar applies.
  - Property names and values in `\p{...}` and `\P{...}` are checked against the pinned Unicode 18.0.0 data (`D-036`). The development generator is extended to read `PropertyValueAliases.txt` for General_Category and Script values. The property name tables of the ECMAScript specification (tables 65 to 67) are written by hand and cite their source.
  - The increment selected after this one was the arena-based syntax tree and start of the expression parser (E1), now implemented as an initial bounded expression subset. A token driver and throughput measurement were not prioritized ahead of it.
- **Rationale:** A file with an invalid regular expression has a syntax error and must become `unknown` (`D-034`), so lexical scanning alone would let such files pass as valid. Real Next.js code runs in browsers and Node.js, which accept Annex B patterns; rejecting them would wrongly mark ordinary files as `unknown`. Building the tree next tests the central E1 design question early.
- **Tradeoffs:** The increment is larger than the previous scanners. Annex B adds a second grammar with different handling of braces, octal escapes, control escapes, and UTF-16 code units. Property validation adds a second pinned data file with a recorded hash. Throughput evidence for E1 arrives later than with a token driver first.
- **Implemented contract:** `scan_js_regexp` returns only the lexical extent (span, body, and flags). `validate_js_regexp` rescans and checks flags and pattern, returning typed flags and the capturing group count, or a typed error with a closed `RegExpPatternError` reason. The separation lets a future parser skip an invalid literal at a known boundary (`D-034`). Details are in section 4.7 of [IMPLEMENTATION.md](IMPLEMENTATION.md).
- **Unicode data:** `PropertyValueAliases.txt` 18.0.0, downloaded on 2026-10-12 with 83,536 bytes and the SHA-256 hash `06c4c8eaf7b0bf34abe73b113da1215bd784ac254d4c223600b90267caa4bbbd`. Like `DerivedCoreProperties.txt`, it is downloaded deliberately for each regeneration and is not committed.
- **Revisit if:** the ECMAScript specification adds flags or pattern syntax, the Unicode version changes, or evidence shows that Annex B acceptance hides real defects in analyzed projects.

### D-038: Expression grammar completion with in-place pattern reinterpretation

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Claude Code question interface.
- **Context:** The initial expression parser (`D-037`) covers a bounded subset. The next E1 increment could complete the expression grammar, start statement and source-file parsing, or add a measurement harness first. The representation of destructuring targets and the handling of context-dependent names were also open.
- **Decision:**
  - Complete the expression grammar that does not need function bodies: array and object literals (properties, shorthand properties, computed keys, spread), spread arguments, assignment with all compound and logical operators, destructuring through the cover grammar, prefix and postfix update, `new` with and without arguments, and arrow functions with expression bodies. Methods, accessors, generator and async forms, arrow functions with block bodies, and function and class expressions remain fail-closed `UnsupportedSyntax` until statement parsing exists. Statement parsing and a measurement harness follow later.
  - Destructuring and assignment targets use their own node kinds (array and object patterns, pattern properties, rest elements, default values, binding identifiers). The parser first builds the cover expression and, once `=` or `=>` decides its role, reinterprets the same arena nodes in place, keeping their `NodeId`s. Assignment targets keep identifier references; arrow parameters become binding identifiers.
  - Names and constructs whose validity depends on the source goal, strict mode, or function context stay fail closed with typed errors: `await`, `yield`, strict-mode reserved words, async arrows, `new.target`, `eval` or `arguments` as targets, and `delete` of an identifier. The parser API gains no context parameter yet.
- **Rationale:** Expressions are the largest part of the grammar that statements, JSX, and TypeScript build on, and the cover grammar is the main remaining design question of the arena (E1). Distinct pattern kinds let exhaustive matches separate reads from writes and bindings, which scope analysis in M4 needs (`D-033`). In-place reinterpretation avoids copying subtrees and leaves no unreachable nodes. Deferring context avoids fixing an API before the file parser knows the source goal.
- **Tradeoffs:** Code that is valid only in sloppy mode or needs function context is rejected for now, so the parser accepts less than a complete one would. Expression-only early errors (shorthand initializers outside patterns, duplicate `__proto__`) are checked after the complete expression is parsed, so a later syntax error can be reported before them.
- **Implemented contract:** `parse_js_expression` covers this grammar with new expression, pattern, and binding node kinds, `AssignmentOperator`, and `UpdateOperator`. Call expressions as targets of `=`, compound assignment, or update are also reported as `StrictModeRequiresContext`, because ECMAScript treats them as web-compat targets that are valid outside strict mode. A differential comparison with V8 on generated fictional expressions found no accepted input that the specification rejects. Details are in section 4.8 of [IMPLEMENTATION.md](IMPLEMENTATION.md).
- **Revisit if:** statement parsing introduces source goals and function context, or E1 evidence shows that in-place reinterpretation complicates recovery (`D-034`) or tree consumers. `D-039` introduces source goals and function context for full source files and keeps the context-free contract of `parse_js_expression`.

### D-039: Complete JavaScript grammar with explicit source goals and full sloppy mode

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Claude Code question interface. Refines the context handling that `D-038` deferred.
- **Context:** `D-038` completed the expression grammar without function bodies and deferred statements, source goals, and function context. The scope of the next E1 increment, the handling of Script and Module source text, the place of scope-dependent early errors, and the order of the following increments were open.
- **Decision:**
  - One increment covers the complete ECMAScript syntactic grammar except JSX and TypeScript: Script and Module source text, all statements and declarations (`var`, `let`, `const`, `using`, `await using`), functions of every form (declarations and expressions; async, generator, and async generator functions; arrow functions with block bodies; async arrow functions; object methods, getters, and setters), classes (methods, accessors, fields, private names, static blocks, `extends`, and `super`), import and export declarations with import attributes, `import()`, `import.meta`, `new.target`, `yield`, `await` including top-level await, automatic semicolon insertion, directive prologues, and hashbang comments.
  - A new full-source entry point takes an explicit source goal, Script or Module. Module code is always strict. Script code supports complete sloppy mode, including the web-compatibility grammar of Annex B: HTML-like comments, legacy octal and non-octal decimal literals and escapes, `with`, labelled function declarations, function declarations in `if` clauses, duplicate block-level function declarations, initializers in `for-in` heads, `var` redeclaration of catch parameters, and call expressions as assignment targets. Words that only strict code reserves, such as `let`, `static`, and `yield`, are identifiers where sloppy code permits them. Strictness follows directive prologues, including the retroactive effect of `"use strict"` on a function's name, parameters, and earlier directives.
  - `parse_js_expression` keeps its context-free contract. It may accept function and class expressions, but every construct whose validity depends on the unknown source goal or strictness still fails closed.
  - All early errors of this grammar are reported while parsing, including redeclarations, label sets, `break` and `continue` targets, `return` placement, duplicate and undeclared exports, private names, `super` and `new.target` placement, and class element rules. Scope-dependent checks use a transient scope stack that the parser discards after parsing. The semantic layer (M4) builds its own scope graph from the tree.
  - The following E1 increment is a measurement harness for throughput and peak memory on generated fictional source files, then JSX, then TypeScript. `D-040` inserts a throughput increment between the harness and JSX.
- **Rationale:** A source file is the unit that analysis classifies, so file-level parsing is required before any corpus evidence exists. Next.js projects contain CommonJS configuration files that run as sloppy scripts, and the project owner chose to support them completely instead of failing closed. Annex B describes what browsers and Node.js accept; rejecting it would mark valid files `unknown`, and parsing a script with module rules would silently misread HTML-like comments. Reporting early errors while parsing keeps erroneous files out of transformations (`D-034`), and a transient scope stack keeps binding semantics in the semantic layer. Measuring before JSX and TypeScript exposes design problems of the arena while the grammar is still small enough to change.
- **Tradeoffs:** This is the largest increment so far, so its review and differential validation take longer. Sloppy mode and Annex B add lexical modes to the string and numeric scanners, a goal parameter to the trivia scanner, and many context-dependent rules. Scope-dependent early errors are tracked in the parser and again by the later M4 scope analysis, which duplicates some name bookkeeping. The full-source parser still returns one error without recovery (`D-034`).
- **Implemented contract:** `parse_js_program` takes a `SourceGoal` and returns a tree rooted in a `Program` node; `parse_js_expression` keeps its context-free contract. The trivia scanner takes the source goal, and the string and numeric scanners take a `LegacyOctal` mode that reports legacy forms, so the parser applies strictness once it is known. The parser moved to `src/parser/` with separate modules for expressions, patterns, functions, classes, statements, modules, identifier rules, and transient scopes. Where V8 rejects source text that the specification accepts, the parser rejects it as well, because no engine could load the file: `<!--` in module code and `await` in static field initializers, for which the specification's grammar states no early error. Where V8 accepts text that the specification rejects, the parser follows the specification. A differential comparison with V8 on generated fictional programs found no other disagreement. Details are in sections 4.8 to 4.12 of [IMPLEMENTATION.md](IMPLEMENTATION.md).
- **Revisit if:** the measurement harness shows that the arena or the transient scope tracking misses the E1 budgets, ECMAScript adds syntax (for example decorators or new import phases reaching Stage 4), the specification clarifies `await` in static field initializers, or evidence shows that Annex B acceptance hides real defects in analyzed projects.

### D-040: Built measurement harness with a counting allocator, then a throughput increment before JSX

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Claude Code question interface. Refines the increment order of `D-039`. Confirmed again on the same day in a second interview of a later run, which began before the harness was visible to it: the project owner chose the same counting allocator, built harness, and generated corpus, and directed that the run optimize the parser until it meets the throughput budget with margin.
- **Context:** `D-039` selected a measurement harness for throughput and peak memory on generated fictional source files as the next E1 increment. How to measure peak memory, where the corpus comes from, and whether to build or adopt a benchmark harness were open. A rough preliminary measurement outside the repository, on an Intel Core i9-14900K, parsed module code at about 9.4 MB/s from 16 KB to 4 MB per file: linear in size, but about a third of the E1 budget of 30 MB/s per core.
- **Decision:**
  - A development-only binary, `tools/frameshift-bench`, measures parser throughput and peak heap memory through the engine facade. It uses its own small harness on the standard library: a warm-up round, fixed iteration counts, minimum and median times, and a scaling series over file sizes. Statistics-based benchmark libraries such as criterion are not adopted now; benchmark tooling is evaluated again for the M11 benchmarks.
  - Peak heap memory is measured with a counting global allocator that forwards to the system allocator. Implementing `GlobalAlloc` requires unsafe code, so this crate alone declares its own lints: `unsafe_code` is `deny` instead of `forbid`, a single allocator module allows it, and Clippy's `undocumented_unsafe_blocks` requires a safety comment on every unsafe block. The other workspace lints apply unchanged, and product crates keep `forbid(unsafe_code)`.
  - The corpus comes from a deterministic, purpose-built generator with a fixed seed and its own pseudo-random number generator. It emits fictional, module-shaped JavaScript in fixed sizes. Every generated file must parse, or the harness fails. Generated files are not committed; a fingerprint identifies the corpus across platforms. Reading real files from a directory is deferred until JSX and TypeScript can be parsed.
  - The harness is committed with its measured baseline. The following increment improves parser throughput, with before-and-after measurements, an unchanged public API, and identical trees. JSX follows it, then TypeScript.
- **Rationale:** Only the allocator observes the exact heap peak including transient parser state; capacity accounting from a finished tree would understate it, and operating-system peak-memory queries need platform-specific foreign calls. Generated input controls file sizes and the construct mix, contains no third-party code, and is reproducible. A small harness suffices to expose a threefold gap and to show linear scaling, while criterion would add many transitive dependencies and still not measure memory. Improving throughput before JSX keeps the change small while the parser has fewer token contexts (`D-033`).
- **Tradeoffs:** The bench crate contains reviewed unsafe code. The counting allocator adds an atomic update to every allocation, also in timed runs. Heap figures exclude stack, allocator overhead, and fragmentation, so they are lower than resident memory. Generated code approximates real projects but does not replace them; real files are measured once JSX and TypeScript exist. Timing depends on hardware and is noisy on hybrid processors, so budgets are checked on the reference hardware ([ENGINEERING.md](ENGINEERING.md), section 4.1).
- **Revisit if:** heap figures diverge substantially from resident memory measured by the operating system, M11 benchmark trends need statistical analysis, or real-world files show a construct mix that the generator does not represent.

## Technique proposals and related accepted decisions

### P-001: Go as the single implementation language, CGO disabled

- **Status:** Superseded by D-018 (2026-10-11). Kept for the record.
- **Proposal:** Implement engine and CLI in Go only. CGO is disabled as a hard rule, the standard library comes first, and foreign code is used, if ever, only across a process boundary.
- **Rationale (original):** Go's standard library, single-host cross-compilation, compatibility promise, and simplicity, with performance shown by the Go-based TypeScript compiler and esbuild. The re-evaluation in section 2.3 of [ENGINEERING.md](ENGINEERING.md) gave more weight to the long-term multi-language scope.
- **Tradeoffs:** No sum types or exhaustive pattern matching, higher memory use than Rust, and the parser has to be built.
- **Main alternative:** Rust as the single language, either with purpose-built parsers or with oxc behind a facade.

### P-002: Purpose-built parsers

- **Status:** Proposed (2026-10-11). Depends on E1.
- **Proposal:** Build Frameshift's own parsers for JavaScript, TypeScript, JSX, TSX, CSS and SCSS, JSONC, the YAML subset used by lockfiles, and the Yarn classic lockfile format.
- **Rationale:** Parsing is the most correctness-critical component. Frameshift needs control over spans, comments, the error policy, and resource limits, and a tree design that extends to further languages. The strongest Rust parser, oxc, is still pre-1.0 with regular breaking AST changes.
- **Fallback if E1 reveals a fundamental problem:** oxc behind a Frameshift-owned facade, with its own decision record. oxc may serve as a development-time test oracle in any case.

### P-003: Span-based edits and targeted printers; no concrete syntax tree initially

- **Status:** Proposed (2026-10-11). Depends on E2.
- **Proposal:** A span-precise abstract syntax tree plus the original text. Code changes are non-overlapping span edits, and new files come from printers that embed source slices. A lossless concrete syntax tree is the fallback.
- **Rationale:** Preserves formatting and comments in unchanged code with less machinery than a concrete syntax tree.

### P-004: MVP project model instead of a universal IR

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** V1 uses a project model sized for one migration path, with neutral naming and framework facets. V2 extracts a common core plus facets once a second source or target exists. See section 10 of [ARCHITECTURE.md](ARCHITECTURE.md).
- **Rationale:** Neutral concepts and explicit facets prepare extension while preserving execution-model details. A universal IR needs semantic evidence from further frameworks (`D-033`).

### P-005: Phase separation and recomputed plans

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** Analysis is read-only, planning is pure, execution writes only to staging, and verification runs external tools only when authorized. `migrate --plan` recomputes the plan and requires an identical hash. The plan file is an approval artifact and is never executed.
- **Rationale:** Inspectable plans, trivial dry-runs, no plan tampering, and determinism.

### P-006: Fixed kind-based weights (`weights-v1`)

- **Status:** Accepted (2026-10-11), confirmed by the project owner. The weight values are still validated in M5 (Q-002).
- **Proposal:** Weighted headline metrics with fixed integer weights per unit kind, alongside unweighted counts. Dependencies and verbatim files are excluded from headline metrics and reported separately. See section 5 of [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).
- **Tradeoff:** Coarse, because size within a kind is ignored, but explainable and comparable across runs.

### P-007: Conversion ladder for components

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** Astro component, then static Preact component, then Preact island, then preserve. Astro conversion is computed as a fixed point over render contexts. See section 9 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md).
- **Rationale:** Keeps V1's Astro conversion subset narrow without breaking behavior.

### P-008: `preact/compat` as the default island strategy

- **Status:** Accepted (2026-10-11), confirmed by the project owner.
- **Proposal:** Keep source React imports and alias `react` and `react-dom` to `@preact/compat`. Native Preact rewriting is a later optimization that requires a verified mapping for every API, attribute, and event.
- **Rationale:** Minimal code changes, and preserved React semantics (for example `onChange` on text inputs).
- **Tradeoff:** A small runtime overhead compared with native Preact.

### P-009: `client:load` as the default hydration directive

- **Status:** Accepted (2026-10-11), confirmed by the project owner.
- **Proposal:** Islands hydrate with `client:load`, which is closest to Next.js hydration timing. `client:idle`, `client:visible`, and `client:media` are opt-in optimizations, reported separately.

### P-010: Lockfile carry-over instead of lockfile generation

- **Status:** Accepted (2026-10-11), confirmed by the project owner.
- **Proposal:** Never generate lockfiles. Copy the source lockfile unchanged, keep declared ranges for retained dependencies, use target profile versions for new dependencies, and add a manual task to run an install.
- **Rationale:** Generating a lockfile requires registry resolution over the network. Carrying the old lockfile over lets the package manager keep existing resolutions.

### P-011: Output metadata and support file locations

- **Status:** Accepted (2026-10-11), confirmed by the project owner with a changed summary location.
- **Decision:** `.frameshift/manifest.json`, `.frameshift/report.json`, `.frameshift/unmigrated/`, the human-readable summary `.frameshift/SUMMARY.md`, and generated helpers in `src/frameshift-compat/`. The CLI points to the summary at the end of every migration. Generated source files carry no header comments.
- **Rationale for the summary location:** keeps the project root clean.

### P-012: Exit code scheme

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** 0 success, 1 condition failed, 2 usage, 3 input, 4 output refused, 5 completed with failures, 6 environment, 70 internal error, 130 interrupted, with defined precedence. See section 7 of [CLI.md](CLI.md).

### P-013: Verification levels

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface with runtime moved into V1.
- **Decision:** `structural`, `build`, `static-output`, and `runtime` are cumulative V1 levels. Runtime checks cover hydration, declared scripted interactions, and selected accessibility-tree comparisons, under explicit execution authorization with a user-installed browser. Each level states what it demonstrates and what it does not. See section 16 of [ARCHITECTURE.md](ARCHITECTURE.md).
- **Sequencing:** Implement runtime in M10 after migration and executable fixtures exist, as confirmed in the follow-up interview. Browser tooling and the interaction-check protocol are decided in M10. M23 extends runtime across V2 adapters and targets rather than introducing it for the first time.

### P-014: Release workflow structure

- **Status:** Accepted (2026-10-11), confirmed by the project owner.
- **Proposal:** Release Please on pushes to `main`. Packaging runs as a reusable workflow, called in the same run when a release was created, to work around `GITHUB_TOKEN` trigger limits. Manual dispatch is available for reruns. CI on release pull requests runs automatically through short-lived tokens from a GitHub App installed only on this repository. A draft-release flow will be used once immutable releases are enabled (E5). See [RELEASES.md](RELEASES.md).

### P-015: Artifact naming and platform tiers

- **Status:** Accepted (2026-10-11), confirmed by the project owner.
- **Proposal:** `frameshift-<version>-<os>-<arch>.<ext>` plus a SHA-256 checksum file. Tier 1: Linux x86_64 and ARM64, Windows x86_64 and ARM64, macOS ARM64. Tier 2: macOS x86_64. Linux binaries are statically linked against musl. See section 7 of [RELEASES.md](RELEASES.md).

### P-016: Finding codes and identifier schemes

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** Finding codes `FS` followed by four digits, grouped by area. Rule IDs `<migration-path-id>/<area>.<name>`. Feature IDs `<ecosystem>.<category>.<name>`. Unit IDs `<kind>:<path or URL pattern>[#<export>]`. See [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md) and section 8 of [ENGINEERING.md](ENGINEERING.md).

### P-017: Minimal relocation policy for output layout

- **Status:** Superseded by D-024 (2026-10-11).
- **Proposal:** Only files whose role requires it move: routes to `src/pages/`, layouts to `src/layouts/`, and stylesheets and assets colocated in `app/` to `src/styles/` and `src/assets/`. Everything else keeps its relative path. See section 16 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md).
- **Tradeoff:** Fewer import rewrites and stable content paths, at the cost of an unconventional layout for projects without a `src/` directory.

## Deferred decisions

| ID | Topic | Why deferred | Revisit when |
| --- | --- | --- | --- |
| DF-001 | Configuration file (name, format) | Resolved by D-027 | Resolved |
| DF-002 | Plugin architecture | No third-party rule authors yet. Adds security and compatibility surface. | M24 in V2 |
| DF-003 | License | The repository is private. The choice affects dependency compatibility. | Before going public |
| DF-004 | Code signing, notarization, provenance attestations | Attestations need GitHub Enterprise Cloud for private repositories. Signing has costs. | Public phase |
| DF-005 | Model-assisted processing | Deterministic engine first (D-006) | V3 track T10, subject to [LLM_POLICY.md](LLM_POLICY.md) |
| DF-006 | GUI and editor integrations | CLI-only scope (D-004) | V3 track T9 |
| DF-007 | Pages Router transformation | Keeps V1 focused on the App Router | M17 in V2 |
| DF-008 | Review acknowledgment ledger | Needs a stable report and review model first | M22 in V2 |
| DF-009 | Incremental analysis cache and resumable execution | Not justified at V1 project sizes | M21 in V2 |
| DF-010 | Astro client router (view transitions with persisted islands) | Changes navigation behavior. Needs careful equivalence analysis. | M20 in V2 |
| DF-011 | Second migration path | Target side resolved by D-031 (Astro + React). The second source framework still requires V1 evidence and demand. | V1 to V2 transition |
| DF-012 | WebAssembly build target | No current requirement | A browser-based or embedded frontend becomes a goal |

## Open questions

| ID | Question | Current thinking | Resolve by |
| --- | --- | --- | --- |
| Q-001 | Column encoding in source locations | Resolved by D-029 | Resolved |
| Q-002 | Final values of `weights-v1` | Validate the proposed weights on real projects | M5 |
| Q-003 | Naming of public environment variables | Resolved by D-021 | Resolved |
| Q-004 | Several components in one file | Resolved by D-025 | Resolved |
| Q-005 | Static output comparison: normalization, thresholds, HTML parser (build, or adopt an established HTML5 parser such as html5ever) | Experiment E4 | M10 |
| Q-006 | Real-world evaluation corpus | Process resolved by D-030. The concrete project list is proposed and approved in M5. | M5 |
| Q-007 | Target profile policy | Resolved by D-028 | Resolved |
| Q-008 | Monorepo scope in V1 | Resolved by D-022 | Resolved |
| Q-009 | Placement of modules colocated in `app/` | Resolved by D-024 | Resolved |
| Q-010 | Exhaustiveness checks for closed kinds | Resolved by D-018: compiler-enforced exhaustive matches without wildcard arms (section 2.5 of [ENGINEERING.md](ENGINEERING.md)) | Resolved |
| Q-011 | Parser error recovery depth | Policy resolved by D-034: bounded recovery at safe grammar boundaries; erroneous files remain ineligible for transformation. Concrete synchronization points and default budgets remain M3 work. | Policy resolved; implementation in M3 |
| Q-012 | Lowest supported Next.js version | Resolved by D-023 | Resolved |

## Experiments

| ID | Experiment | Decides | Milestone |
| --- | --- | --- | --- |
| E1 | Parser and tree design in Rust: syntax coverage, spans, throughput, memory, fuzzing, maintainability of the arena design | P-002 | M0 |
| E2 | Edit-based rewriting and Astro printing on three fixtures | P-003 | M0 |
| E3 | Static evaluation coverage of real `next.config.*` and `tailwind.config.*` files | Scope of configuration rules | M4 |
| E4 | Static output comparison of Next.js export and Astro build | Q-005 | M10 |
| E5 | Draft and immutable release flow with Release Please in a private repository | P-014 details, DF-004 preparation | M11 |

Detailed success criteria for E1 and E2 are in section 2.8 of [ENGINEERING.md](ENGINEERING.md). Experiment results are recorded here together with the decisions they inform.

**E1 progress (2026-10-12):** Parser-directed JavaScript trivia, strict-string, numeric, punctuation, template, and identifier scanners are implemented in the framework-independent syntax crate, exposed through the engine facade, and tested with fictional inputs. Exact spans, comment ordering, newline evidence, typed errors, tagged invalid-escape evidence, and explicit local limits are covered. This prepares parser-controlled lexical goals without guessing JSX, regular-expression, or template contexts. The template increment was selected through the question interface (`D-035`). Identifier characters are now classified with tables generated from pinned Unicode 18.0.0 data, which replaced the conservative numeric boundary error (`D-036`). Regular-expression literals are scanned and completely validated, including Annex B without the `u` or `v` flag and property names from pinned Unicode data (`D-037`). A recursive-descent parser and flat syntax-node arena cover the expression grammar, including destructuring through in-place cover-grammar reinterpretation (`D-038`), and complete Script and Module source text with statements, functions, classes, modules, sloppy mode, Annex B, and all early errors through transient scopes, with source spans, comments, and explicit resource limits (`D-039`). JSX and TypeScript are not parsed yet. The measurement harness (`D-040`) parses a generated 10.3 MB corpus at 11.5 MB/s on an Intel Core i9-14900K, below the budget of 30 MB/s, with 9.7 times the source size in heap when all trees are retained, within the budget of 20 (section 6 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). The throughput increment is next. No standalone token stream, corpus pass rate, reference-hardware result, or coverage-guided fuzzing result exists yet. P-002 remains Proposed, and E1 has not passed. Contracts and validation are recorded in [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Template for new entries

```markdown
### <ID>: <Title>

- **Status:** Proposed | Accepted | Deferred | Superseded by <ID> (<date>).
- **Context:** What problem or question prompted this.
- **Decision or proposal:** What is decided or proposed.
- **Rationale:** Why.
- **Tradeoffs:** What is given up.
- **Revisit if:** Conditions that would reopen it.
```
