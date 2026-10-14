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
| D-041 | JSX by the specification grammar with an explicit dialect, and pinned character reference tables | Accepted |
| D-042 | Integrated TypeScript and TSX parsing with structured type nodes | Accepted |
| D-043 | E1 evidence from a TypeScript corpus and an approved real-world sample | Accepted |
| D-044 | Purpose-built coverage-guided fuzzing on stable Rust | Accepted |
| D-045 | E1 memory budget applies to the retained whole corpus | Accepted |
| D-046 | Go helper for processor-time measurement | Accepted |
| D-047 | E1 closure: accepted maintainability review and throughput on the development machine | Accepted |
| D-048 | E2 structure: an output-layer printer crate, a development driver, a pinned Astro build, and Linux test runs | Accepted |
| D-049 | E2 closure and the end of M0 | Accepted |
| D-050 | M1 CLI shell and truthful version documents | Accepted |
| D-052 | Go development helper for deterministic release archives | Accepted |
| P-001 | Go as the single implementation language, CGO disabled | Superseded by D-018 |
| P-002 | Purpose-built parsers | Accepted |
| P-003 | Span-based edits and targeted printers; no concrete syntax tree initially | Accepted |
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
| Q-001 to Q-013 | Open questions | Open or resolved (see table) |
| E1 to E5 | Experiments | E1 and E2 passed; others planned |

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
- **Ninth one-run exception (2026-10-12):** Through the Claude Code chat, the project owner explicitly authorized committing and pushing directly to `main` for the `D-041` JSX and character reference increment and its documentation, once the agent is satisfied with the work. This exception has the same limits as the first one.
- **Tenth one-run exception (2026-10-12):** Through the Codex chat, the project owner explicitly authorized inspecting the worktrees, completing the run interrupted by a usage limit, and committing and pushing its verified changes directly to `main`. Inspection confirmed that the `D-041` run had already pushed all four implementation commits before the interruption. This run revalidates that state, reconciles the superseded uncommitted `D-036` regex draft without replacing the implemented `D-037` contract, and updates the documentation. This exception has the same limits as the first one.

- **Eleventh one-run exception (2026-10-12):** Through the Codex chat, the project owner explicitly authorized committing and pushing the verified `D-042` TypeScript and TSX increment and its documentation directly to `main`, once the agent is satisfied with the work. This exception has the same limits as the first one.

- **Twelfth one-run exception (2026-10-12):** Through the Asker chat, the project owner explicitly authorized completing the interrupted `D-042` run and committing and pushing its verified TypeScript and TSX increment and documentation directly to `main`, once the agent is satisfied with the work. This exception has the same limits as the first one.

- **Thirteenth one-run exception (2026-10-13):** Through the Claude Code chat, the project owner explicitly authorized committing and pushing directly to `main` for the `D-043` increment, the `D-044` decision, and their documentation, once the agent is satisfied with the work. This exception has the same limits as the first one.

- **Fourteenth one-run exception (2026-10-13):** Through the Codex chat, the project owner explicitly authorized continuing development, recording the interview decisions, and committing and pushing verified changes directly to `main` once the agent is satisfied with the work. This run continues the TypeScript corpus work with parser contract checks and implements `D-045`. This exception has the same limits as the first one.

- **Fifteenth one-run exception (2026-10-13):** Through the Claude Code chat, the project owner explicitly authorized continuing development, recording the interview decisions, and committing and pushing directly to `main` once the agent is satisfied with the work. This run implements the `D-044` fuzzer, its first budget runs, the parser fixes that they led to, and the draft of the E1 maintainability review. This exception has the same limits as the first one.

- **Sixteenth one-run exception (2026-10-13):** Through the Claude Code chat, the project owner explicitly authorized continuing development, recording the interview decisions, and committing and pushing directly to `main` once the agent is satisfied with the work. This run implements the follow-up work of the E1 maintainability review, the `D-046` helper, the span comparison with the pinned TypeScript compiler, and the fuzzing budget run under `D-047`. This exception has the same limits as the first one.

- **Seventeenth one-run exception (2026-10-13):** Through the Claude Code chat, the project owner explicitly authorized continuing development, recording the interview decisions, and committing and pushing directly to `main` once the agent is satisfied with the work. This run implements experiment E2 under `D-048` and records its closure in `D-049`. This exception has the same limits as the first one.

- **Eighteenth one-run exception (2026-10-14):** In the same Claude Code session, the project owner asked to verify that E1 and E2 are complete, to review the maintainability of the project, and to commit and push a short fix of any small gap directly to `main`. This run fixes unbounded recursion in the JSX conversion, adds the `astro` fuzz target, and records the review in section 11.1 of [IMPLEMENTATION.md](IMPLEMENTATION.md). This exception has the same limits as the first one.

- **Nineteenth one-run exception (2026-10-14):** Through the Codex chat, the project owner explicitly authorized continuing development, interviewing for outstanding decisions, and committing and pushing verified changes directly to `main` once the work is complete. This run starts M1 with the grouped engine facade and the help and version CLI. This exception has the same limits as the first one.

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
- **Throughput increment outcome (2026-10-12):** Five measured steps raised parse throughput on the default corpus from 13.0 to 68.4 MB/s on the same machine: punctuator candidates indexed by first byte, caches for repeated lookahead at one offset, byte-level trivia and ASCII identifier scanning with sorted reserved-word keys, reused child-list buffers, and an inlined check for already skipped trivia. The public API is unchanged, a development-time comparison of about one million outcomes found identical trees and errors, peak heap is unchanged, and optimized builds use about 1.4 instead of 1.3 KiB of stack per nesting level. Details are in section 6 of [IMPLEMENTATION.md](IMPLEMENTATION.md). JSX was the next increment; `D-041` implements it.
- **Revisit if:** heap figures diverge substantially from resident memory measured by the operating system, M11 benchmark trends need statistical analysis, or real-world files show a construct mix that the generator does not represent.

### D-041: JSX by the specification grammar with an explicit dialect, and pinned character reference tables

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Claude Code question interface. Implements the JSX increment that `D-040` scheduled.
- **Context:** JSX was the next E1 increment. Open were: whether the parser follows the JSX specification or the stricter React compilers for constructs such as namespaced names (`<svg:rect>`, `xlink:href`) and spread children (`{...items}`); how HTML character references in JSX text and attribute strings are handled, which do not affect syntactic validity but do affect a future Astro printer (React compilers decode 253 XHTML names, while Astro templates are HTML, which decodes the 2,231 WHATWG names); how callers enable JSX; and which development-time oracle replaces V8, which has no JSX.
- **Decision:**
  - The parser accepts the complete grammar of the [JSX specification](https://facebook.github.io/jsx/), checked on 2026-10-12: elements and fragments as primary expressions; identifier, namespaced, and member element names; identifier and namespaced attribute names; string, expression container, element, and fragment attribute values; spread attributes; text; expression containers, empty containers, and spread children. Whether React, Preact, or Astro support a construct (Babel's React JSX transform rejects namespaced names by default and spread children always) is decided by later rules and reported as findings. The syntax layer stays free of framework knowledge.
  - As in `D-039`, where the reference compilers reject source text that the grammar accepts, the parser rejects it too, because no compiler would load the file: a `<` token directly after a JSX element or fragment expression, as in adjacent elements without a wrapper, which the grammar would read as a comparison. Where Babel accepts text that the grammar rejects, the parser follows the grammar: a spread child holds an AssignmentExpression, so `{...a, b}` is an error.
  - Callers select the language variant with a closed `JsDialect` enum, currently `JavaScript` and `Jsx`, passed explicitly to `parse_js_program` and `parse_js_expression`. TypeScript variants will extend it. The parser never infers the dialect from the content. Mapping file names and project configuration to a dialect, for example `.js` files that contain JSX in Next.js projects, belongs to the project layer (M2, M4). The signature change is a breaking API change, acceptable before 1.0.0.
  - The parser does not decode character references: JSX text and attribute strings keep their original spans. Two pinned tables are generated in the same increment for later value decoding and printing: the 253 XHTML 1.0 entity names that Babel, TypeScript, and SWC decode in JSX, from the W3C XHTML 1.0 DTD entity files, and the 2,231 named character references of the WHATWG HTML standard (`entities.json`), which Astro's HTML templates follow. A standard-library-only development tool generates them. The source files are downloaded deliberately and not committed; their sizes and hashes are recorded here.
  - `@babel/parser` 8.0.7, installed with npm outside the repository, is the development-time oracle for differential comparison. It is not part of the repository, of any test, or of the build.
- **Rationale:** The JSX specification is the only vendor-neutral grammar, and Frameshift migrates JSX to more than one target, so target support belongs in rules that can explain it. Rejecting adjacent elements matches every compiler and keeps a common authoring error from being misread as a comparison. An explicit dialect keeps the parser deterministic and leaves file classification to the layer that knows the project, and one enum prepares TypeScript and TSX (`D-033`). The tables are generated now so that both decoding semantics are pinned and reviewable before a printer depends on them. Babel is the reference JSX parser of the ecosystem and reports precise errors.
- **Tradeoffs:** The parser accepts constructs that the React compilers used by Next.js reject later, so rules must classify them. The adjacency rule is stricter than the grammar. The API change touches every caller. Two pinned tables add notice obligations for the W3C and WHATWG sources before any distribution ([SECURITY.md](SECURITY.md), section 11). Babel is not SWC, which Next.js uses; differences between them remain unmeasured.
- **Implemented contract:** `scan_jsx_identifier`, `scan_jsx_string`, and `scan_jsx_text` scan JSX names (with hyphens, without escapes), attribute strings (without escapes), and text (up to `{` or `<`, rejecting `>` and `}`) at parser-selected offsets, with a new `max_jsx_text_bytes` limit. `parse_js_program` and `parse_js_expression` take a `JsDialect`; `Program` nodes record it. Fifteen JSX node kinds keep exact spans, and closing tags must match their opening names by structure and spelling. Details are in section 4.13 of [IMPLEMENTATION.md](IMPLEMENTATION.md). `decode_jsx_character_reference` decodes one reference as the JSX compilers do, with the XHTML table, and `decode_html_character_reference` as the WHATWG tokenizer does in text or attribute context, with the HTML table; both read a caller-selected offset of any text and allocate nothing. `tools/frameshift-entity-gen` generates `entity_tables.rs` and checks it with `--check`. Details are in section 4.14 of [IMPLEMENTATION.md](IMPLEMENTATION.md). The measurement harness (`D-040`) gained a generated JSX component corpus, on which the parser met the E1 throughput and memory budgets on the development machine (section 6 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).
- **Source data:** Downloaded on 2026-10-12: `entities.json` from `https://html.spec.whatwg.org/entities.json`, 145,897 bytes, SHA-256 `d741d877ac77c4194c4ad526b5b4a19aef8dfe411ab840a466891cdbb9f362e6`; from `https://www.w3.org/TR/xhtml1/DTD/`, `xhtml-lat1.ent`, 11,775 bytes, SHA-256 `3da1daad08f93b8aff7a32d5447f94853d517561d9c7d0c7bc3ee5c002d12a38`; `xhtml-symbol.ent`, 13,848 bytes, SHA-256 `a6cd0f9c59e488b8c8e2de80c31c6811f316a3919cd9a0725b5390a8fc785d61`; and `xhtml-special.ent`, 4,131 bytes, SHA-256 `7f9518026c59ccee47a02ed74e43f8946d92207d438a7f540a0f3c28f05fa27e`. The HTML Standard is licensed under CC BY 4.0, and the XHTML sets carry an ISO 8879 notice; both notices are repeated in the generated file.
- **Revisit if:** the JSX specification changes, evidence from SWC or real projects shows a different accepted language, or a target needs the parser to decode values.

### D-042: Integrated TypeScript and TSX parsing with structured type nodes

- **Status:** Accepted (2026-10-12), confirmed by the project owner through the Codex App question interface.
- **Decision:** Implement TypeScript and TSX source-file parsing together, including the type grammar, declarations, and extensions to expressions, functions, classes, and modules. Extend the explicit `JsDialect`; never infer a dialect from source text. Represent types with structured, closed node kinds and exact source spans in the existing arena, rather than opaque spans. Type checking, inference, module resolution, and compiler-option-dependent semantic checks remain outside the syntax layer.
- **Reference:** Pin Microsoft's official JavaScript TypeScript compiler to `typescript` 6.0.3 as a development-time syntax oracle. Its `createSourceFile` API and `parseDiagnostics` define the TypeScript grammar, including TSX generic-arrow disambiguation. This version preserves the stable compiler API needed by the oracle, whereas the native TypeScript 7 API is still unstable. Install it outside the repository, with scripts disabled; it is neither a runtime dependency nor a build input. The compiler is Apache-2.0, maintained by Microsoft, and replaceable as an oracle without changing the engine API.
- **Rationale (`D-033`):** Integrating the grammar exercises actual TS/TSX file contexts and shared JavaScript constructs immediately. Structured types support future declared-type analysis in M15 and source-preserving edits without committing to a type checker. The shared arena and visitor keep all syntax traversable with bounded storage and preserve the E1 experiment gate.
- **Tradeoffs:** TypeScript's authoritative grammar is its compiler implementation rather than a current formal specification. Syntax acceptance does not prove type correctness or target compatibility. Speculation around generics and arrows needs deterministic work limits and rollback of arena state. E1 remains open until corpus, reference-hardware, fuzzing, and maintainability evidence exists.
- **Implemented contract:** `JsDialect` gains `TypeScript` and `Tsx`, and `Program` nodes record the dialect. `crates/frameshift-syntax/src/typescript.rs` defines the closed `TypeScriptKind` node set and `src/parser/typescript.rs` parses the type grammar, declarations, and extensions inside the shared arena, with speculation limited to type syntax and function heads, rollback of arena and scope state, and a `ts_work` bound that counts consumed tokens during speculation. Since 2026-10-13, every speculation runs at most once per position and parser state, which removes exponential work on nested parenthesized expressions and type arguments that the fuzzer exposed (`D-044`). The instantiation reading of `a < b > c` follows `canFollowTypeArgumentsInExpression` of the oracle, a property access directly on an instantiation expression is a parse error, and heritage clauses keep their type arguments without the comparison ambiguity. Frameshift keeps the ECMAScript early errors of invalid assignment and update targets as parse errors in these dialects, where the oracle's parser accepts most of them and its checker reports them later; this, and the single-`>` type-argument closing, are the recorded deviations. The dialects report no redeclaration early errors, because TypeScript declares overloads and merges declarations while a genuine duplicate is a checker diagnostic, and the oracle's parser accepts every case of the comparison battery. Details are in section 4.15 of [IMPLEMENTATION.md](IMPLEMENTATION.md), and the oracle comparisons with their results are in section 5.

### D-043: E1 evidence from a TypeScript corpus and an approved real-world sample

- **Status:** Accepted (2026-10-13), confirmed by the project owner through the Claude Code question interface, including the project list below.
- **Context:** After `D-042`, experiment E1 still lacked throughput and memory measurements for the TypeScript dialects, the sample of real-world Next.js App Router files that section 2.8 of [ENGINEERING.md](ENGINEERING.md) requires (at least 99 percent of files parse, and every failure is explained by syntax not yet implemented), fuzzing, and a maintainability review. `D-030` governs the evaluation corpus only from M5 on, so how the E1 sample is selected was open, as were the order of the remaining E1 work and the fuzzing approach (`D-044`).
- **Decision:**
  - The next increment extends the development-only measurement harness `tools/frameshift-bench` (`D-040`) in two ways: a deterministic generated corpus of TypeScript modules and TSX components, and a read-only directory mode that parses the JavaScript and TypeScript files of a directory outside the repository and reports the parse rate, every file that fails with its position and error kind, and throughput and peak memory on the accepted files. This ends the deferral of real files in `D-040`. The directory mode maps file name extensions to dialects and source goals itself; that mapping belongs to the tool and does not anticipate the project layer's file classification (M2, M4).
  - The E1 sample follows the `D-030` process early: the agent proposes public Next.js App Router projects with permissive licenses (MIT, Apache-2.0, BSD, or ISC) and a reason for each, and the project owner approves the list. Approved projects are fetched at pinned commits into a cache outside the repository, never committed, and never built, installed, or executed; they are only parsed. The repository records the repository, commit, and license of each project.
  - `typescript` 6.0.3 and `@babel/parser` 8.0.7 remain development-time oracles (`D-041`, `D-042`), installed with npm outside the repository with install scripts disabled. They explain rejections on the sample and check the generated TypeScript corpus. They are not part of the repository, any test, or the build.
  - Fuzzing follows in its own increment under `D-044`. The maintainability review closes E1 after that.
- **Approved sample (2026-10-13):** about 13,300 JavaScript and TypeScript files, 36 MB. Next.js versions are the declared dependencies at the pinned commits.

  | Repository | Commit | License | Next.js | Reason |
  | --- | --- | --- | --- | --- |
  | `vercel/next.js`, `examples/` only | `e7feae7d53c1b62741d8fe6b6adb429dc669079d` | MIT | mixed | 226 official examples with the broadest construct mix, including `.js` and `.jsx` |
  | `vercel/examples` | `ecee7812ac6f28d7f169445fae155fac6d503c18` | MIT | 16.x | Many small App Router applications in JavaScript and TypeScript |
  | `vercel/commerce` | `3761e52e60df9c6a316e067dbfd7032e494d3634` | MIT | 15.6 | Reference commerce application |
  | `vercel/next-learn` | `bb2558441a6673ab76c89914c25018bffa27a2ba` | MIT | 14.x and latest | Official course with a dashboard application |
  | `nextjs/saas-starter` | `6e33e58b1e553a41fe22e6b941a7229a002de361` | MIT | 15.6 | App Router starter |
  | `timlrx/tailwind-nextjs-starter-blog` | `b45bef66b40c63b6f57c15ee8cd090682238df4c` | MIT | 15.5 | Static blog with `.mjs` and `.cjs` configuration |
  | `shadcn-ui/ui` | `30a58d4498f270af1c1e981a7d78a15cf5740841` | MIT | 16.3 and 16.4 | Large TSX component corpus in App Router applications |
  | `shadcn-ui/taxonomy` | `298a8857c7128a0d121e7f699dfd729f23b3966d` | MIT | 13.3 | Early App Router application, outside the `D-023` range, used as parser evidence only |
  | `steven-tey/precedent` | `3be40205d7cdf56082cd284f07f12251b9208f79` | MIT | 14.3 | App Router starter |
  | `fuma-nama/fumadocs` | `abe12092fa27d4d61209944d5c95052e50dde101` | MIT | 16.3 | Static documentation framework and site, TypeScript-heavy |
  | `shuding/nextra` | `d6e80e1dd627b781429a6ee989b15ebba688c8ea` | MIT | 16.x | Static documentation on the App Router, with `.jsx` and `.mts` |
  | `heroui-inc/heroui` | `88a39c793a5c5545eb1f3cb966524c726d6716c7` | Apache-2.0 | 16.4 | Component library with an App Router documentation application, TSX-heavy |

- **Rationale:** Real files are the strongest available evidence for the parse-rate criterion and expose grammar gaps that generated and hand-picked inputs miss. Pinned commits make the sample reproducible, permissive licenses allow local analysis, and keeping the files outside the repository respects `D-012` and the rule that committed fixtures are fictional. A generated TypeScript corpus measures the TypeScript dialects under the same controlled conditions as the existing corpora (`D-040`).
- **Tradeoffs:** The sample is weighted toward popular templates, component libraries, and documentation sites, and several repositories also contain library code and test fixtures that are not Next.js application files. Fetching needs network access once. A pinned commit identifies content but cannot guarantee that a public repository stays available.
- **Implemented contract and results (2026-10-13):** `frameshift-bench --dir <path>` and the generated TypeScript corpus are described in section 6 of [IMPLEMENTATION.md](IMPLEMENTATION.md). On the approved sample, 13,282 of 13,292 files (99.92 percent) parse, one of them only with raised limits; each of the 10 rejected files is explained, 9 are rejected by the oracles as well, and no file that Frameshift accepts is rejected by an oracle. Throughput on the accepted files is 93.5 MB/s and the retained trees take 8.7 times the source size on the development machine. The sample and the corpus exposed seven grammar differences from the pinned compiler, all fixed with regression tests (section 4.15 of [IMPLEMENTATION.md](IMPLEMENTATION.md)): literal types before an arrow function body, declaration files, modifiers after `static`, object types in template substitutions, type arguments directly followed by `=`, assertion assignment targets, and `<<` at the start of an expression. Declaration files need an ambient context that the parser cannot infer from content, so `JsDialect` gained `TypeScriptDeclaration`, following the explicit-dialect contract of `D-041` and `D-042`; the caller maps `.d.ts`, `.d.mts`, and `.d.cts` files to it. The TypeScript corpus parses at 49.0 MB/s with 12.4 times its size retained. Per file, the peak heap stays under 20 times the source size on every generated corpus, but not for a few tiny real files, where fixed allocations dominate. `D-045` resolves `Q-013` by applying the budget to the retained whole and leaving per-file peaks as diagnostics.
- **Revisit if:** M5 replaces the sample with the `D-030` evaluation corpus, a pinned commit becomes unavailable, or a new sample parses below 99 percent.

- **Follow-up scope (2026-10-13):** Through the Codex App question interface, the project owner selected continuing the TypeScript/TSX corpus work with parser checks and documentation. The fetched `main` already contained the generated corpus, so the follow-up adds `frameshift-bench --check` for reproducible parser and empty-edit invariants across all generated dialects. It preserves the generator version and every input fingerprint, remains behind the public engine facade, and leaves timing, compiler span comparisons, fuzzing, and migration verification as separate evidence.

### D-044: Purpose-built coverage-guided fuzzing on stable Rust

- **Status:** Accepted (2026-10-13), confirmed by the project owner through the Claude Code question interface. Updated on 2026-10-13 with the feasibility result, a second unsafe location, and the E1 budget, which the project owner confirmed through the Claude Code question interface before the implementation increment.
- **Context:** E1 requires fuzzing for a fixed CPU budget without crashes or hangs, section 3.3 of [TESTING.md](TESTING.md) requires fuzzing of every parser of untrusted input, and section 3.5 of [ENGINEERING.md](ENGINEERING.md) asked for an evaluation of coverage-guided tools such as cargo-fuzz. cargo-fuzz needs a nightly toolchain, a C++ compiler to build libFuzzer, and the `libfuzzer-sys` crate, and its Windows support is limited. A fuzzer without coverage feedback needs no unsafe code or new tooling but reaches deep parser states less reliably.
- **Decision:** Build a development-only, coverage-guided fuzzer as a tool in the workspace. Coverage comes from LLVM SanitizerCoverage instrumentation, enabled through code generation options of the pinned stable `rustc` (`-C passes=sancov-module` with `-C llvm-args`) and read by a small purpose-built runtime. Inputs come from a purpose-built mutator seeded with the generated corpora and fictional fixtures. The runtime's callbacks need unsafe code, which is confined to one reviewed module of the tool under the rules of the counting allocator (`D-040`). The first step checks feasibility on the development platforms. If the instrumentation is not usable with the pinned stable toolchain, the fallback is a standard-library-only generation and mutation fuzzer without coverage feedback, recorded as an update to this decision.
- **Rationale:** Coverage feedback finds parser states that random generation misses. Stable instrumentation keeps the pinned toolchain (`D-032`) and adds no dependency (`D-013`), and a purpose-built runtime also works on Windows, where development happens.
- **Tradeoffs:** Building a runtime and mutator costs more than adopting libFuzzer (`D-020`). The instrumentation options are low-level contracts of `rustc` and LLVM that can change with the toolchain. The tool contains unsafe code.
- **Feasibility (2026-10-13):** A probe outside the repository, on Windows x86_64 with the pinned Rust 1.98.0, confirmed that `-Cpasses=sancov-module` with `-Cllvm-args=-sanitizer-coverage-level=3` and `-Cllvm-args=-sanitizer-coverage-inline-8bit-counters` works on the stable toolchain under two conditions. First, every crate must be compiled as one code generation unit (`-Ccodegen-units=1`): with several units, rustc's local ThinLTO step removes functions after instrumentation, and LLVM aborts because the counter arrays keep COMDAT associations with the removed functions. Second, on Windows, the section markers `__start___sancov_cntrs` and `__stop___sancov_cntrs`, which compiler-rt normally supplies, must be defined in a unit without instrumented functions; in an instrumented module, LLVM renames its own references to them and linking fails. With both conditions met, the module constructors registered one counter region, and the counters followed the input. Linux and macOS linkers define such markers themselves. The instrumentation was not validated there, because no Linux or macOS environment was available.
- **Second unsafe location (2026-10-13):** The project owner accepted a separate development-only crate, `tools/frameshift-fuzz-sections`, that holds only the two marker statics with the unsafe attributes `no_mangle` and `link_section`. It has no functions, so it is never instrumented, and no unsafe blocks, and it is empty outside Windows. Together with the runtime module of the fuzzer, it is the reviewed unsafe code of the fuzzing tool. Product crates keep `forbid(unsafe_code)`.
- **E1 budget (2026-10-13):** Fuzzing passes the E1 criterion when every fuzz target runs for one CPU-hour without a crash, hang, or contract violation. The planned targets are the program parser in every dialect and source goal, the expression parser, regular-expression scanning and validation, the lexical scanners, the character reference decoders, and the span edit core.
- **Implemented contract (2026-10-13):** `tools/frameshift-fuzz`, built with `tools/frameshift-fuzz/instrument.toml`, and `tools/frameshift-fuzz-sections`, as described in section 7 of [IMPLEMENTATION.md](IMPLEMENTATION.md). The runtime clears and copies the counters around each execution, features follow libFuzzer's hit-count buckets, and a deterministic mutator with a dictionary per target starts from small files of the generated corpora and fictional snippets. The six targets above check documented contracts; the program and expression targets reuse the contract checks of `tools/frameshift-corpus`, which now also cover rejections. A supervising process runs the fuzzing loop in a worker process of the same executable, treats its status lines as heartbeats, and saves the recorded input of every crash, hang, or contract violation. A separate process captures stack overflows, aborts, and exhausted memory, which the standard library offers no portable hook for, and keeps a watchdog thread from running instrumented code in the worker while counters are measured.
- **Results (2026-10-13):** Two runs of all six targets found no crash or contract violation. They exposed exponential speculation in the TypeScript dialects, which rejected valid nested code: once through the slowest-input diagnostic and once as a hang. Both are fixed with regression tests (`D-042`, section 4.15 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). Neither run satisfies the E1 budget: in this agent session, processes received far less processor time than wall-clock time, about 970 CPU-seconds per worker in 3,720 seconds, so the budget run after the fixes, with measured CPU time, is still open (section 7 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).
- **Order (2026-10-13):** Through the Claude Code question interface, the project owner chose to leave the budget run after the speculation fixes open for now and to write the E1 maintainability review next, as a draft for the owner's confirmation.
- **Budget run (2026-10-13, `D-046`, `D-047`):** On the development desktop, every target ran for 4,000 seconds of wall-clock time with more than 3,940 seconds of measured processor time and found nothing on the final commit. On the way, the `program` and `expression` targets found two span defects of the TypeScript dialects with the new token boundary contract; their analysis exposed a third span defect and a function expression accepted without a body. All are fixed with regression tests (section 7 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). The E1 fuzzing criterion is met.
- **Revisit if:** the feasibility check fails, an established coverage-guided fuzzer becomes usable on stable Rust and Windows without a C++ build, or the purpose-built fuzzer misses defects that an established fuzzer finds.

### D-045: E1 memory budget applies to the retained whole corpus

- **Status:** Accepted (2026-10-13), confirmed by the project owner through the Codex App question interface. Resolves Q-013.
- **Context:** The approved sample keeps its trees in 8.7 times its source size, but a 10-byte file reaches a 54.6 times per-file peak because fixed allocations dominate tiny inputs. The earlier budget did not say whether it applied to each file or the whole corpus.
- **Decision:** The E1 budget is a peak heap demand of at most 20 times the total source bytes during the pass that keeps every source text and syntax tree of a corpus alive. Apply it separately to each generated corpus and the accepted real-world sample. Continue reporting per-file median and maximum peaks, the worst file's source size and absolute heap demand, and retained ratios as diagnostics, without a per-file pass/fail verdict. An all-empty corpus has no comparable ratio and receives no budget verdict.
- **Rationale:** Full corpus retention models the engine's planned analysis workload. Retaining visibility into tiny-file costs keeps the fixed overhead reviewable without making the ratio depend on the smallest file.
- **Tradeoffs:** A corpus-wide ratio can hide an expensive outlier. Per-file diagnostics and explicit parser limits remain necessary. The counting allocator measures requested heap bytes, not resident memory, allocator overhead, or fragmentation (`D-040`). Reference-hardware measurements remain required.

### D-046: Go helper for processor-time measurement

- **Status:** Accepted (2026-10-13), chosen by the project owner through the Claude Code question interface, with reference to the language evaluation in section 2 of [ENGINEERING.md](ENGINEERING.md).
- **Context:** The E1 fuzzing budget is one CPU-hour per target (`D-044`), but the fuzzer's budget is wall-clock time, because the Rust standard library has no clock for processor time. The first budget runs sampled processor time with ad hoc commands outside the repository, and processes started from the agent session on the laptop received far less processor time than wall-clock time. The alternatives were external sampling as before, a processor-time budget inside the fuzzer that reads `GetProcessTimes` through Rust foreign-function declarations (a further unsafe location under `D-044`), or a small helper in a language whose standard library reads processor time natively.
- **Decision:** `tools/frameshift-cputime` is a development-only Go program that uses only the Go standard library, without the `unsafe` package or cgo. It runs a program, follows the processes that the program starts, reads their user and kernel time at a fixed interval and once more at the end, writes a versioned JSON Lines log, prints a summary, and exits with the program's exit code. On Windows it reads the Toolhelp process snapshot and the times of a handle that it keeps open for every tracked process, so the times of ended processes stay exact; on Linux on amd64 and arm64 it reads `/proc`. On other platforms it builds but refuses to run.
- **Why not Rust (section 2.7 of [ENGINEERING.md](ENGINEERING.md)):** The Rust standard library has no processor-time API, so Rust would need hand-written foreign-function declarations of Win32 functions and structures in unsafe code, or an adopted crate such as `windows-sys` or `libc`. Go's standard library provides typed, maintained bindings (`syscall.GetProcessTimes`, `syscall.CreateToolhelp32Snapshot`, `os.ProcessState.UserTime`) and cross-compiles for every tier 1 target from one host, so the helper is memory-safe without unsafe code or dependencies. Go is the documented alternative language of `D-018`. Evidence: the helper itself, tested on Windows x86_64 and on Linux x86_64 (a WSL2 kernel), and cross-compiled for Linux x86_64 and ARM64, Windows x86_64 and ARM64, and macOS ARM64.
- **Alternatives considered:** ad hoc sampling scripts (not a tested, reproducible artifact); Rust foreign-function declarations (unsafe code, declined by the project owner); Python (its standard library reports the processor time of child processes only on POSIX systems); .NET (`Process.TotalProcessorTime` is portable, but it needs the .NET SDK and runtime, and .NET was not part of the language evaluation); C (memory-unsafe, with a compiler toolchain per platform).
- **Requirements of section 2.7:** a separate process with a versioned protocol: the command line and the log format `frameshift-cputime` version 1 with `header`, `sample`, and `end` records. It reads only the operating system's process table, writes only a new log file, which it never overwrites, and starts only the given program. It is never shipped and builds with the pinned Go toolchain on every tier 1 platform. Measurements are timing evidence, not engine output; a failed sample marks the run incomplete, and the helper then exits with code 125 instead of the program's code. Tests, `gofmt`, and `go vet` apply. No product crate or command depends on it.
- **Toolchain:** Go 1.25.5, the toolchain installed on the development machine and a supported release line, pinned by the `go` directive of `tools/frameshift-cputime/go.mod`. `GOTOOLCHAIN=go1.25.5` selects exactly this version where another one is installed. A deliberate change of the directive replaces the pin.
- **Tradeoffs:** Developers who run budget runs need a second toolchain. macOS is not supported at run time, because the Go standard library cannot read the times of another process there without unsafe code. On Linux, a process that ended between samples keeps its last sampled times unless a sample saw it as a zombie, so its reported time is a lower bound by at most one interval; on both platforms, a process that starts and ends within one interval is missed.
- **Revisit if:** the Rust standard library gains a processor-time API, the fuzzer needs a processor-time budget of its own (for example in CI), or budget runs are needed on macOS.

### D-047: E1 closure: accepted maintainability review and throughput on the development machine

- **Status:** Accepted (2026-10-13), confirmed by the project owner through the Claude Code question interface.
- **Context:** The draft E1 maintainability review (section 8 of [IMPLEMENTATION.md](IMPLEMENTATION.md)) awaited the project owner's confirmation. The throughput criterion was to be checked on reference hardware, an eight-core laptop from 2023 or later ([ENGINEERING.md](ENGINEERING.md), section 4.1), but the laptop measurements were disturbed by processor throttling of the agent session and by background synchronization.
- **Decision:**
  - The maintainability review is accepted. The follow-up work it recommended is done in the same run: the parser state that a TypeScript speculation depends on and restores is classified in one place, from which the speculation record key and the rollback derive, and the four wildcard arms over an `Option` of a closed enum became membership tests. Splitting the TypeScript grammar module (risk 2) and typed accessors for TypeScript children (risk 3) remain follow-ups for when the grammar next grows and when M4 consumes types.
  - The E1 throughput and memory criteria are judged on the development desktop (Intel Core i9-14900K), together with the earlier observations on the managed virtual machine. Reference-hardware checks of all performance budgets, including parser throughput, take place before releases in M11, as section 4.3 of [ENGINEERING.md](ENGINEERING.md) provides. The budgets themselves are unchanged.
  - The remaining E1 evidence in this run is the span comparison of the TypeScript dialects with the pinned compiler and the fuzzing budget run with processor time measured by `D-046`.
- **Rationale:** A desktop measurement under controlled conditions is reproducible evidence for the design question that E1 answers, linear parsing with sufficient headroom, while measurements under throttling are not. The reference-hardware check stays mandatory before a release.
- **Tradeoffs:** The desktop is faster per core than the reference laptop, so the TypeScript dialects, which parse at about 70 percent of the speed of JavaScript, may still miss 30 MB/s on reference hardware. That risk is accepted until M11, where a miss leads to a throughput increment before 1.0.0.

### D-048: E2 structure: an output-layer printer crate, a development driver, a pinned Astro build, and Linux test runs

- **Status:** Accepted (2026-10-13), confirmed by the project owner through the Claude Code question interface.
- **Context:** E1 passed (`D-047`), so experiment E2 was the remaining M0 gate. Open were where its code lives, how much of the printer it builds, how the criterion "Astro builds the output with the pinned target profile" is checked without making Astro part of the repository, and how byte identity across operating systems is shown on a Windows development machine.
- **Decision:**
  - A product crate of the output layer, `crates/frameshift-emit`, holds the Astro component model and printer, the conversion of React JSX into that model, and emitted text with multi-source provenance, so that M7 builds on it. A development-only tool, `tools/frameshift-e2`, wires the transformations of the three E2 fixtures by hand: it anticipates neither the rule engine nor the planner (M6, M7).
  - The JSX conversion covers the subset of section 9.3 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md) in general, not only what the fixtures use, and fails closed with a typed error on everything else.
  - The target profile `astro7-preact10` is installed with npm outside the repository, with install scripts disabled, and `astro build` runs only on the generated fictional outputs. The versions are those of 2026-10-13: `astro` 7.3.8, `@astrojs/preact` 6.0.6, `preact` 10.29.8, and `@preact/compat` 18.3.2 as the `react` and `react-dom` aliases. Preact 11.0 became stable on 2026-09-30, but `@astrojs/preact` 6.0.6 declares `preact` `^10.6.5` as its peer, so `D-028` keeps Preact 10. The resulting `package-lock.json` had 162,184 bytes and the SHA-256 hash `082ff1022bdc505faecd94630b0aa67f4a0ea1421bb3684d4453065763e8e84e`. Astro is a development-time check, not part of the repository, any test, or the build.
  - The pinned Rust 1.98.0 gains the `x86_64-unknown-linux-musl` standard library through `rustup target add`. Test binaries are linked statically with the bundled `rust-lld` and run on Linux x86_64 in a WSL distribution against the same golden files. macOS follows with the M1 CI.
- **Rationale (`D-033`):** A printer that only a throwaway tool contained would have to be rebuilt in M7, while hand-wired fixture transformations keep the experiment from fixing rule and plan contracts before M5 and M6 define them. A general converter tests the design question of E2, whether span-precise trees and targeted printers suffice, on the whole V1 subset. Building with the real compiler is the only way to confirm that the output is valid Astro. Static musl binaries need no Linux toolchain on the development machine.
- **Tradeoffs:** The E2 tool duplicates some analysis that rules will own later and is replaced by them. The Astro check needs network access once and a Node.js installation, and it executes Astro's code on Frameshift-generated fictional projects only. Linux runs cover x86_64 with musl, not glibc or ARM64.
- **Implemented contract:** sections 10 and 11 of [IMPLEMENTATION.md](IMPLEMENTATION.md).
- **Revisit if:** the target profile changes (`D-028`), M7 replaces the hand-wired transformations with rules, or M1 CI provides native runs on every tier 1 platform.

### D-049: E2 closure and the end of M0

- **Status:** Accepted (2026-10-13), confirmed by the project owner through the Claude Code question interface.
- **Context:** The E2 run met every criterion of section 2.8 of [ENGINEERING.md](ENGINEERING.md) on Windows x86_64 and Linux x86_64. No macOS machine was available. M0 also lists the setup of branch protection on `main`, which would end the one-run direct pushes of `D-010`.
- **Decision:**
  - E2 passed, and `P-003` is accepted: span-precise syntax trees with the per-file comment list, span edits, and targeted printers suffice for V1; no concrete syntax tree is needed. Byte identity on macOS is shown by the cross-platform CI of M1.
  - Branch protection is set up together with the CI status checks of M1, not in M0. With E1 and E2 passed and `P-002` and `P-003` accepted, M0 is complete, and M1 is next.
- **Rationale:** Windows and Linux already differ in path separators, line-ending defaults, and the C runtime, and both produced identical bytes; the printer reads no platform state. Branch protection without required checks would add no assurance.
- **Tradeoffs:** A macOS-specific difference would surface only in M1. Until M1, `main` stays unprotected and relies on the integration workflow of `D-010`.

### D-050: M1 CLI shell and truthful version documents

- **Status:** Accepted (2026-10-14), with the version document contract confirmed by the project owner through the Codex App question interface.
- **Context:** M0 is complete. M1 needs a usable CLI shell, and the E2 maintainability review calls for grouping the engine facade before the CLI consumes it. No product migration ruleset, knowledge base, or target profile exists yet.
- **Decision:** Add `crates/frameshift-cli`, with the executable `frameshift`, depending only on the engine facade and the standard library. Implement `help`, command help, `version`, `--version`, and `--format text|json` for version. Reject unavailable commands and unimplemented options. Keep the engine's `foundation`, `syntax`, `transform`, and `emit` namespaces and the existing M0 root imports. Do not add dependencies or change the unreleased Cargo package version `0.0.0`.
- **Version document:** `frameshift.version`, `schemaVersion: 1`, with `engineVersion`, `rulesets`, `knowledgeBases`, `targetProfiles`, `buildCommit`, and `toolchain`. The three catalogs are empty arrays until product data ships. `buildCommit` is `null` until release tooling reliably embeds it. `toolchain` is the actual compiler identity captured by a standard-library-only Cargo build script invoking Cargo's `RUSTC --version`; normal operation executes nothing and reads no build metadata from the host. No Git inspection, timestamps, or absolute paths are embedded. The schema is `schemas/frameshift.version.schema.json`.
- **Errors and streams:** Usage errors exit with 2, write a static English diagnostic to standard error, and, when JSON was explicitly requested, write one `frameshift.error` v1 document containing `exitCode` and `message` to standard output. User-supplied argument text is never echoed. The error schema is `schemas/frameshift.error.schema.json`. A failed standard-stream write or flush exits with 6; a failed stream cannot guarantee a complete JSON document.
- **Validation:** Process-level golden and repeatability tests cover aliases, option placement, invalid arguments, non-Unicode native arguments, streams, and independence from runtime environment and current directory. Unit tests cover JSON string escaping and output failures. Golden updates are an explicitly ignored test mode, never part of ordinary test runs.
- **Rationale:** A finished shell makes the engine boundary concrete without pretending that migration commands are ready. Accurate compiler metadata and empty catalogs prevent development experiments from being advertised as supported product capabilities. The small argument contract and fixed document shapes fit purpose-built standard-library code under section 3 of [ENGINEERING.md](ENGINEERING.md). Layer namespaces anticipate additional subsystems without forcing simultaneous changes to the development tools (`D-033`).
- **Tradeoffs:** This increment does not finish M1. Foundation findings, stable identifiers, portable paths, hashing, shared limits and deterministic collections, automated crate-boundary checks, CI, packaging, Release Please, and branch protection remain outstanding. The build script must be able to invoke the compiler that Cargo selected. The initial JSON writer handles shell documents only; general project documents remain reporting-layer work.
- **Revisit if:** Release Please introduces the first release version and reliable commit metadata, the shipped catalogs become nonempty, or reporting needs a shared document writer.

### D-052: Go development helper for deterministic release archives

- **Status:** Accepted (2026-10-14), with the Go choice confirmed through the Codex App question interface.
- **Decision:** Add `tools/frameshift-release`, a development-only Go 1.25.5 module using only the standard library. It builds sorted tar.gz and ZIP archives with UTC commit timestamps, normalized modes and ownership, empty gzip name/time headers, verifies every archived entry's bytes and mode, and produces sorted SHA-256 checksum lists. All output files are created exclusively; existing artifacts are never overwritten. Inputs must be regular files bounded to 256 MiB each. Only the declared artifact directory is written. The helper runs no binary itself.
- **Evidence and alternatives (`D-019`):** Go provides `archive/tar`, `archive/zip`, `compress/gzip`, and the audited `crypto/sha256` implementation in its standard library on all target platforms. Rust's standard library provides none of these; equivalent Rust code needs separately evaluated libraries, and Frameshift must not build cryptography or compression (`ENGINEERING.md`, section 3.4). Platform tar/zip tools have divergent metadata defaults. The owner chose the memory-safe standard-library helper over additional Rust libraries.
- **Boundary and distribution:** A separate development process, not a Cargo workspace member or product dependency. Its explicit flags describe a built binary, repository notices, artifact location, semantic version, platform, architecture, and commit epoch. Rust remains the only product implementation language. Native CI tests and vets the Go module and checks gofmt. Tests cover repeated archives, byte preservation, executable modes, checksums, malformed options, tampered entries, foreign files, and refusal to overwrite.
- **Tradeoffs:** Release builds require the pinned Go toolchain as well as Rust. The helper holds a small fixed set of bounded files in memory, and Go's compression implementation is a build input. No license for Frameshift itself is selected by packaging third-party notices.
- **Revisit if:** Release tooling needs streaming large assets or Rust adopts maintained compression and hash libraries for another accepted requirement.

## Technique proposals and related accepted decisions

### P-001: Go as the single implementation language, CGO disabled

- **Status:** Superseded by D-018 (2026-10-11). Kept for the record.
- **Proposal:** Implement engine and CLI in Go only. CGO is disabled as a hard rule, the standard library comes first, and foreign code is used, if ever, only across a process boundary.
- **Rationale (original):** Go's standard library, single-host cross-compilation, compatibility promise, and simplicity, with performance shown by the Go-based TypeScript compiler and esbuild. The re-evaluation in section 2.3 of [ENGINEERING.md](ENGINEERING.md) gave more weight to the long-term multi-language scope.
- **Tradeoffs:** No sum types or exhaustive pattern matching, higher memory use than Rust, and the parser has to be built.
- **Main alternative:** Rust as the single language, either with purpose-built parsers or with oxc behind a facade.

### P-002: Purpose-built parsers

- **Status:** Accepted (2026-10-13). E1 passed under the criteria of section 2.8 of [ENGINEERING.md](ENGINEERING.md) and the closure rules of `D-047`, which the project owner confirmed; the purpose-built JavaScript, JSX, TypeScript, and TSX parser is the foundation for M3. The other parsers of this proposal are built under the same policy when their milestones arrive.
- **Proposal:** Build Frameshift's own parsers for JavaScript, TypeScript, JSX, TSX, CSS and SCSS, JSONC, the YAML subset used by lockfiles, and the Yarn classic lockfile format.
- **Rationale:** Parsing is the most correctness-critical component. Frameshift needs control over spans, comments, the error policy, and resource limits, and a tree design that extends to further languages. The strongest Rust parser, oxc, is still pre-1.0 with regular breaking AST changes.
- **Fallback if E1 reveals a fundamental problem:** oxc behind a Frameshift-owned facade, with its own decision record. oxc may serve as a development-time test oracle in any case.

### P-003: Span-based edits and targeted printers; no concrete syntax tree initially

- **Status:** Accepted (2026-10-13). E2 passed under `D-049`, which the project owner confirmed.
- **Proposal:** A span-precise abstract syntax tree plus the original text. Code changes are non-overlapping span edits, and new files come from printers that embed source slices. A lossless concrete syntax tree is the fallback.
- **Rationale:** Preserves formatting and comments in unchanged code with less machinery than a concrete syntax tree.
- **Evidence (E2):** Rewritten files are byte-exact outside their edits, printed files copy expressions, statements, and comments as source slices, and the per-file comment list together with node spans accounts for every comment, so dropping one is detected and rejected. What a concrete syntax tree would add, trivia attached to tokens, was needed nowhere: canonical printing of markup regenerates tags anyway, and comments inside a regenerated tag fail closed instead of being moved.

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
| Q-013 | E1 memory budget: per file or retained whole | Resolved by D-045: peak heap demand of the whole corpus with all source texts and trees retained; per-file peaks remain diagnostics | Resolved |

## Experiments

| ID | Experiment | Decides | Milestone |
| --- | --- | --- | --- |
| E1 | Parser and tree design in Rust: syntax coverage, spans, throughput, memory, fuzzing, maintainability of the arena design | P-002 | M0 |
| E2 | Edit-based rewriting and Astro printing on three fixtures | P-003 | M0 |
| E3 | Static evaluation coverage of real `next.config.*` and `tailwind.config.*` files | Scope of configuration rules | M4 |
| E4 | Static output comparison of Next.js export and Astro build | Q-005 | M10 |
| E5 | Draft and immutable release flow with Release Please in a private repository | P-014 details, DF-004 preparation | M11 |

Detailed success criteria for E1 and E2 are in section 2.8 of [ENGINEERING.md](ENGINEERING.md). Experiment results are recorded here together with the decisions they inform.

**E1 progress (2026-10-13):** Parser-directed JavaScript trivia, strict-string, numeric, punctuation, template, and identifier scanners are implemented in the framework-independent syntax crate, exposed through the engine facade, and tested with fictional inputs. Exact spans, comment ordering, newline evidence, typed errors, tagged invalid-escape evidence, and explicit local limits are covered. This prepares parser-controlled lexical goals without guessing JSX, regular-expression, or template contexts. The template increment was selected through the question interface (`D-035`). Identifier characters are now classified with tables generated from pinned Unicode 18.0.0 data, which replaced the conservative numeric boundary error (`D-036`). Regular-expression literals are scanned and completely validated, including Annex B without the `u` or `v` flag and property names from pinned Unicode data (`D-037`). A recursive-descent parser and flat syntax-node arena cover the expression grammar, including destructuring through in-place cover-grammar reinterpretation (`D-038`), and complete Script and Module source text with statements, functions, classes, modules, sloppy mode, Annex B, and all early errors through transient scopes, with source spans, comments, and explicit resource limits (`D-039`). An explicit JSX dialect adds the complete grammar of the JSX specification, compared with `@babel/parser` on generated fictional programs (`D-041`), and the TypeScript and TSX dialects add the type, declaration, and expression grammar of the pinned compiler with structured type nodes, compared with `typescript` 6.0.3 on generated and hand-picked fictional inputs (`D-042`). The measurement harness (`D-040`) first measured 11.5 MB/s on a generated 10.3 MB corpus on an Intel Core i9-14900K, below the budget of 30 MB/s per core. After the throughput increment, the parser reaches 68.4 MB/s there, with 9.7 times the source size in heap when all trees are retained, within the budget of 20 (section 6 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). These figures come from a desktop processor, not the reference hardware; the JSX increment left them unchanged, and a generated JSX component corpus parses at 70.9 MB/s with 10.5 times the source size retained. The `D-043` increment added a generated TypeScript corpus, which parses at 49.0 MB/s with 12.4 times its size retained, and a sample of 13,292 real files from twelve approved Next.js projects, of which 99.92 percent parse, every rejection is explained, and no accepted file is rejected by the oracles; it parses at 93.5 MB/s with 8.7 times its size retained. Seven grammar differences found on the way are fixed. A coverage-guided fuzzer exists (`D-044`); its first runs found no crash or contract violation and exposed two cases of exponential speculation in the TypeScript dialects, which are fixed. The run of 2026-10-13 completed E1 under `D-047`: the project owner accepted the maintainability review (section 8 of [IMPLEMENTATION.md](IMPLEMENTATION.md)) and its follow-up work was done; 5,594,635 node spans agreed with the pinned TypeScript compiler on generated and real files, with one defect fixed and seven documented representational differences; and a fuzzing budget run measured more than one CPU-hour per target with the `D-046` helper and found nothing on the final commit, after the new token boundary contract had exposed span defects in the TypeScript dialects, which are fixed. On the development desktop, the final parser parses the generated JavaScript, JSX, and TypeScript corpora at 71.7, 71.9, and 47.2 MB/s and the accepted files of the real-world sample at 94.8 MB/s (minimum of five passes), with whole-corpus peaks of 9.7, 10.5, 12.4, and 8.7 times the source size (section 6 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). The corpus-check follow-up validates all 3,000 generated files through the public facade, with byte-identical output across two runs; all 175 current tests pass on Linux x86_64. Whole-corpus peaks are 9.7x for JavaScript, 10.5x for JSX, and 12.4x for the TypeScript corpus on the managed virtual machine, within the `D-045` budget. E1 passed, and P-002 is accepted. Reference-hardware measurements follow in M11 (`D-047`). Contracts and validation are recorded in [IMPLEMENTATION.md](IMPLEMENTATION.md).

**E2 result (2026-10-13, `D-048`, `D-049`):** The output-layer crate `frameshift-emit` and the driver `tools/frameshift-e2` migrate three fictional fixtures, a static page with links, a layout with static metadata and a server component, and a client counter island, to 17 output files. Untouched regions keep their comments and formatting byte for byte: the island file differs from its source only by the removed directive, and every comment of the converted modules is copied, which the driver checks. Repeated runs produce identical outputs and provenance, and all 228 tests, including the byte-for-byte comparison with the golden files, pass on Windows x86_64 and on Linux x86_64 (static musl binaries in WSL). Every output byte maps to a copied source span or to the source construct it was generated for, as a provenance listing per fixture records. `astro build` with the `astro7-preact10` profile builds all three outputs; a review by hand found their HTML to match the markup that the source components render, including the head that Next.js adds (Next.js itself was not run), and the built island hydrated and counted in a browser. The experiment exposed three facts that the printer now encodes: the Astro 7 compiler applies JSX whitespace rules outside `pre` and other whitespace-sensitive elements, it passes character references through to HTML, which decodes names that JSX keeps literal, and it renders values such as a child `true`, numeric `style` values, and boolean values of non-boolean attributes differently from React. Details are in sections 10 and 11 of [IMPLEMENTATION.md](IMPLEMENTATION.md). macOS validation follows with M1 CI.

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
