# Decision Register

This register records Frameshift's architectural and project decisions. Only entries with the status **Accepted** are binding. **Proposed** entries are recommendations that still need confirmation, usually through an experiment or an explicit decision by the project owner. **Deferred** entries are intentionally postponed. **Open questions** have no recommendation yet, or only a tentative one.

Statuses change only through an edit to this file in a pull request. A changed decision keeps its ID. The entry records the new status, the date, and the reason, and older reasoning remains visible in the Git history.

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
| D-015 | Go is an eligible implementation language | Accepted |
| D-016 | Three-stage development strategy | Accepted |
| D-017 | Documentation structure and canonical agent instructions | Accepted |
| P-001 | Go as the single implementation language, CGO disabled | Proposed |
| P-002 | Purpose-built parsers | Proposed |
| P-003 | Span-based edits and targeted printers; no concrete syntax tree initially | Proposed |
| P-004 | MVP project model instead of a universal IR | Proposed |
| P-005 | Phase separation and recomputed plans | Proposed |
| P-006 | Fixed kind-based weights (`weights-v1`) | Proposed |
| P-007 | Conversion ladder for components | Proposed |
| P-008 | `preact/compat` as the default island strategy | Proposed |
| P-009 | `client:load` as the default hydration directive | Proposed |
| P-010 | Lockfile carry-over instead of lockfile generation | Proposed |
| P-011 | Output metadata and support file locations | Proposed |
| P-012 | Exit code scheme | Proposed |
| P-013 | Verification levels | Proposed |
| P-014 | Release workflow structure | Proposed |
| P-015 | Artifact naming and platform tiers | Proposed |
| P-016 | Finding codes and identifier schemes | Proposed |
| P-017 | Minimal relocation policy for output layout | Proposed |
| DF-001 to DF-012 | Deferred decisions | Deferred |
| Q-001 to Q-012 | Open questions | Open |
| E1 to E5 | Planned experiments | Planned |

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

### D-011: Release management with Release Please

- **Status:** Accepted (2026-10-11).
- **Decision:** Release Please derives versions and changelogs from Conventional Commits and maintains a release pull request. Merging it creates the tag and the GitHub Release. Packaging attaches binaries to that release. Version numbers are never edited by hand. The detailed workflow structure is proposed in P-014.

### D-012: Private repository phase

- **Status:** Accepted (2026-10-11).
- **Decision:** The repository stays private. No publication, visibility changes, public mirrors, uploads of project contents to external services, or telemetry without explicit approval. No license is assumed until one is chosen (DF-003). An open-source preparation checklist is in section 11 of [SECURITY.md](SECURITY.md).

### D-013: Minimal external dependencies with build-versus-adopt criteria

- **Status:** Accepted (2026-10-11).
- **Decision:** External libraries are adopted only when they are demonstrably suitable under the criteria in section 3 of [ENGINEERING.md](ENGINEERING.md). Otherwise, purpose-built internal implementations are preferred, within KISS and YAGNI. Runtime dependencies need individual decision records.
- **Tradeoff:** More internal engineering effort in exchange for control, a smaller attack surface, and freedom from external constraints.

### D-014: Capability-based milestones without deadlines

- **Status:** Accepted (2026-10-11).
- **Decision:** The roadmap uses capability-based milestones ordered by technical dependencies, not calendar dates. See [ROADMAP.md](ROADMAP.md).

### D-015: Go is an eligible implementation language

- **Status:** Accepted (2026-10-11).
- **Decision:** Go is a first-class candidate for the implementation, evaluated on equal footing with Rust, Zig, C, C++, Haskell, and others. This entry does *not* select Go. The language selection is P-001.

### D-016: Three-stage development strategy

- **Status:** Accepted (2026-10-11).
- **Decision:** Development proceeds in three stages: V1 (Next.js to Astro migration MVP), V2 (extensible migration engine), and V3 (universal migration platform). Stage contents are planned in [ROADMAP.md](ROADMAP.md) and refined over time.

### D-017: Documentation structure and canonical agent instructions

- **Status:** Accepted (2026-10-11).
- **Decision:** Each topic has one authoritative document under `docs/`, and the other documents link to it instead of repeating it. `AGENTS.md` is the canonical instruction document for all agents. `CLAUDE.md` only points to it. Documentation must always distinguish planned from implemented behavior.

## Proposed decisions

### P-001: Go as the single implementation language, CGO disabled

- **Status:** Proposed (2026-10-11). Depends on experiments E1 and E2.
- **Proposal:** Implement engine and CLI in Go only. CGO is disabled as a hard rule, the standard library comes first, and foreign code is used, if ever, only across a process boundary.
- **Rationale:** Under the dependency policy, Rust's main advantage (reusable JavaScript tooling crates) is largely neutralized. Go's standard library, single-host cross-compilation, compatibility promise, and simplicity fit a long-lived, CLI-first project. Evidence from the Go-based TypeScript compiler and esbuild shows that its performance is sufficient. Full evaluation: section 2 of [ENGINEERING.md](ENGINEERING.md).
- **Tradeoffs:** No sum types or exhaustive pattern matching (mitigated by Q-010 and fail-closed defaults), higher memory use than Rust, and the parser has to be built.
- **Main alternative:** Rust as the single language, either with purpose-built parsers or with oxc behind a facade.
- **Revisit if:** any condition in section 2.9 of [ENGINEERING.md](ENGINEERING.md) occurs.

### P-002: Purpose-built parsers

- **Status:** Proposed (2026-10-11). Depends on E1.
- **Proposal:** Build Frameshift's own parsers for JavaScript, TypeScript, JSX, TSX, CSS and SCSS, JSONC, the YAML subset used by lockfiles, and the Yarn classic lockfile format.
- **Rationale:** Parsing is the most correctness-critical component. Frameshift needs control over spans, comments, the error policy, and resource limits. There is no suitable public parser library in Go.
- **Fallbacks if E1 fails:** Rust with a purpose-built parser, or Rust with oxc behind a facade (a dependency decision record is then required). Forking an existing Go parser (esbuild-derived or TypeScript-derived) is a last resort, because those trees are designed for other purposes.

### P-003: Span-based edits and targeted printers; no concrete syntax tree initially

- **Status:** Proposed (2026-10-11). Depends on E2.
- **Proposal:** A span-precise abstract syntax tree plus the original text. Code changes are non-overlapping span edits, and new files come from printers that embed source slices. A lossless concrete syntax tree is the fallback.
- **Rationale:** Preserves formatting and comments in unchanged code with less machinery than a concrete syntax tree.

### P-004: MVP project model instead of a universal IR

- **Status:** Proposed (2026-10-11).
- **Proposal:** V1 uses a project model sized for one migration path, with neutral naming and framework facets. V2 extracts a common core plus facets once a second source or target exists. See section 10 of [ARCHITECTURE.md](ARCHITECTURE.md).
- **Rationale:** YAGNI, and the impossibility of faithfully representing all execution models in one IR.

### P-005: Phase separation and recomputed plans

- **Status:** Proposed (2026-10-11).
- **Proposal:** Analysis is read-only, planning is pure, execution writes only to staging, and verification runs external tools only when authorized. `migrate --plan` recomputes the plan and requires an identical hash. The plan file is an approval artifact and is never executed.
- **Rationale:** Inspectable plans, trivial dry-runs, no plan tampering, and determinism.

### P-006: Fixed kind-based weights (`weights-v1`)

- **Status:** Proposed (2026-10-11). Values validated in M5 (Q-002).
- **Proposal:** Weighted headline metrics with fixed integer weights per unit kind, alongside unweighted counts. Dependencies and verbatim files are excluded from headline metrics and reported separately. See section 5 of [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).
- **Tradeoff:** Coarse, because size within a kind is ignored, but explainable and comparable across runs.

### P-007: Conversion ladder for components

- **Status:** Proposed (2026-10-11).
- **Proposal:** Astro component, then static Preact component, then Preact island, then preserve. Astro conversion is computed as a fixed point over render contexts. See section 9 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md).
- **Rationale:** Keeps V1's Astro conversion subset narrow without breaking behavior.

### P-008: `preact/compat` as the default island strategy

- **Status:** Proposed (2026-10-11).
- **Proposal:** Keep source React imports and alias `react` and `react-dom` to `@preact/compat`. Native Preact rewriting is a later optimization that requires a verified mapping for every API, attribute, and event.
- **Rationale:** Minimal code changes, and preserved React semantics (for example `onChange` on text inputs).
- **Tradeoff:** A small runtime overhead compared with native Preact.

### P-009: `client:load` as the default hydration directive

- **Status:** Proposed (2026-10-11).
- **Proposal:** Islands hydrate with `client:load`, which is closest to Next.js hydration timing. `client:idle`, `client:visible`, and `client:media` are opt-in optimizations, reported separately.

### P-010: Lockfile carry-over instead of lockfile generation

- **Status:** Proposed (2026-10-11).
- **Proposal:** Never generate lockfiles. Copy the source lockfile unchanged, keep declared ranges for retained dependencies, use target profile versions for new dependencies, and add a manual task to run an install.
- **Rationale:** Generating a lockfile requires registry resolution over the network. Carrying the old lockfile over lets the package manager keep existing resolutions.

### P-011: Output metadata and support file locations

- **Status:** Proposed (2026-10-11).
- **Proposal:** `.frameshift/manifest.json`, `.frameshift/report.json`, `.frameshift/unmigrated/`, `FRAMESHIFT.md` at the output root, and generated helpers in `src/frameshift-compat/`. Generated source files carry no header comments.

### P-012: Exit code scheme

- **Status:** Proposed (2026-10-11).
- **Proposal:** 0 success, 1 condition failed, 2 usage, 3 input, 4 output refused, 5 completed with failures, 6 environment, 70 internal error, 130 interrupted, with defined precedence. See section 7 of [CLI.md](CLI.md).

### P-013: Verification levels

- **Status:** Proposed (2026-10-11).
- **Proposal:** `structural`, `build`, and `static-output` in V1, and `runtime` from V2. Each level states what it demonstrates and what it does not. See section 16 of [ARCHITECTURE.md](ARCHITECTURE.md).

### P-014: Release workflow structure

- **Status:** Proposed (2026-10-11).
- **Proposal:** Release Please on pushes to `main`. Packaging runs as a reusable workflow, called in the same run when a release was created, to work around `GITHUB_TOKEN` trigger limits. Manual dispatch is available for reruns. A draft-release flow will be used once immutable releases are enabled (E5). See [RELEASES.md](RELEASES.md).

### P-015: Artifact naming and platform tiers

- **Status:** Proposed (2026-10-11).
- **Proposal:** `frameshift-<version>-<os>-<arch>.<ext>` plus a SHA-256 checksum file. Tier 1: Linux x86_64 and ARM64, Windows x86_64, macOS ARM64, and Windows ARM64 if the selected toolchain supports it. Tier 2: macOS x86_64. See section 7 of [RELEASES.md](RELEASES.md).

### P-016: Finding codes and identifier schemes

- **Status:** Proposed (2026-10-11).
- **Proposal:** Finding codes `FS` followed by four digits, grouped by area. Rule IDs `<migration-path-id>/<area>.<name>`. Feature IDs `<ecosystem>.<category>.<name>`. Unit IDs `<kind>:<path or URL pattern>[#<export>]`. See [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md) and section 8 of [ENGINEERING.md](ENGINEERING.md).

### P-017: Minimal relocation policy for output layout

- **Status:** Proposed (2026-10-11).
- **Proposal:** Only files whose role requires it move: routes to `src/pages/`, layouts to `src/layouts/`, and stylesheets and assets colocated in `app/` to `src/styles/` and `src/assets/`. Everything else keeps its relative path. See section 16 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md).
- **Tradeoff:** Fewer import rewrites and stable content paths, at the cost of an unconventional layout for projects without a `src/` directory.

## Deferred decisions

| ID | Topic | Why deferred | Revisit when |
| --- | --- | --- | --- |
| DF-001 | Configuration file (name, format) | Flags are enough for V1. A file format adds surface. | Users need persistent options, or V2 recipes need configuration |
| DF-002 | Plugin architecture | No third-party rule authors yet. Adds security and compatibility surface. | M24 in V2 |
| DF-003 | License | The repository is private. The choice affects dependency compatibility. | Before going public |
| DF-004 | Code signing, notarization, provenance attestations | Attestations need GitHub Enterprise Cloud for private repositories. Signing has costs. | Public phase |
| DF-005 | Model-assisted processing | Deterministic engine first (D-006) | V3 track T10, subject to [LLM_POLICY.md](LLM_POLICY.md) |
| DF-006 | GUI and editor integrations | CLI-only scope (D-004) | V3 track T9 |
| DF-007 | Pages Router transformation | Keeps V1 focused on the App Router | M17 in V2 |
| DF-008 | Review acknowledgment ledger | Needs a stable report and review model first | M22 in V2 |
| DF-009 | Incremental analysis cache and resumable execution | Not justified at V1 project sizes | M21 in V2 |
| DF-010 | Astro client router (view transitions with persisted islands) | Changes navigation behavior. Needs careful equivalence analysis. | M20 in V2 |
| DF-011 | Second migration path | Requires V1 evidence and demand | V1 to V2 transition |
| DF-012 | WebAssembly build target | No current requirement | A browser-based or embedded frontend becomes a goal |

## Open questions

| ID | Question | Current thinking | Resolve by |
| --- | --- | --- | --- |
| Q-001 | Column encoding in source locations | Byte offsets are canonical. Columns for humans in Unicode scalar values. Editor protocols often use UTF-16 code units, which matters for later integrations. | M2 |
| Q-002 | Final values of `weights-v1` | Validate the proposed weights on real projects | M5 |
| Q-003 | Naming of public environment variables | Rename to `PUBLIC_` with manual tasks, or keep names through Vite's `envPrefix` (must be validated against the pinned Astro version) | M8 |
| Q-004 | Several components in one file | V1 keeps such files as static Preact. Splitting into several Astro files is a later optimization. | M7 |
| Q-005 | Static output comparison: normalization, thresholds, HTML parser (build or adopt `golang.org/x/net/html`) | Experiment E4 | M10 |
| Q-006 | Real-world evaluation corpus | Static-leaning App Router projects with permissive licenses, kept outside the repository | M5 |
| Q-007 | Target profile policy | Which Astro majors to support at the same time, and when to adopt Preact 11 once stable | M8 |
| Q-008 | Monorepo scope in V1 | Migrate one selected package. Treat siblings as external dependencies. | M2 |
| Q-009 | Placement of modules colocated in `app/` | Private folders keep their paths under `src/pages/`. Other modules go to `_shared/` directories there. Needs validation on real projects. | M6 |
| Q-010 | Exhaustiveness checks for closed kinds (if Go is accepted) | A small purpose-built analysis tool, or an adopted linter after review | M1 |
| Q-011 | Parser error recovery depth | Enough recovery to report several errors. Never transform files with errors. | M3 |
| Q-012 | Lowest supported Next.js version | 14.x as the validated floor. 13.4 to 13.5 possibly on a best-effort basis. | M5 |

## Planned experiments

| ID | Experiment | Decides | Milestone |
| --- | --- | --- | --- |
| E1 | Parser feasibility in Go: syntax coverage, spans, throughput, memory, fuzzing, maintainability of the tree design | P-001, P-002 | M0 |
| E2 | Edit-based rewriting and Astro printing on three fixtures | P-003, and supports P-001 | M0 |
| E3 | Static evaluation coverage of real `next.config.*` and `tailwind.config.*` files | Scope of configuration rules | M4 |
| E4 | Static output comparison of Next.js export and Astro build | Q-005 | M10 |
| E5 | Draft and immutable release flow with Release Please in a private repository | P-014 details, DF-004 preparation | M11 |

Detailed success criteria for E1 and E2 are in section 2.8 of [ENGINEERING.md](ENGINEERING.md). Experiment results are recorded here together with the decisions they inform.

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
