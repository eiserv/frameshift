# Architecture

**Status: planned architecture with M0, M1, and M2 complete and M3 in progress.** Immutable source text, byte spans, derived positions, parser-directed JavaScript scanners, regular-expression literal validation, identifier classification from pinned Unicode tables, character reference decoding with pinned HTML and XHTML tables, pure span rewriting with text provenance, an Astro component printer with React JSX conversion and multi-source provenance, and a JavaScript parser with a flat syntax-node arena for complete Script and Module source text, with explicit JSX, TypeScript, TSX, and declaration-file dialects, are implemented. [IMPLEMENTATION.md](IMPLEMENTATION.md) records the existing modules and exact contracts. The CLI help and version shell uses the grouped engine facade and its immutable build identity (`D-050`); it performs no project I/O. Native CI and the gated release foundation are implemented under `D-051` and `D-052`. The input layer's read-only discovery walker, `.gitignore` matcher, and read-once snapshot store with a framed snapshot identity exist (`D-064`, `D-065`, `D-067`, section 7), and the syntax layer parses JSON and JSONC (`D-068`). Full YAML 1.2 syntax (`D-071`), the Yarn classic lockfile syntax, and npm, Bun, pnpm 9.0, and Yarn classic and Berry lockfile models (`D-069`, `D-071`, `D-072`) exist. CSS tokenization and generic structural parsing are implemented (`D-077`, `D-078`), and the Sass parser begins SCSS and indented Sass statements and SassScript, with structured selectors (`D-079`, `D-081`). Full Sass conformance remains M3 work; its current results must not authorize transformations. The pipeline and all remaining subsystems below are design intent. Identifiers such as `D-006` or `P-003` refer to entries in [DECISIONS.md](DECISIONS.md): "Accepted" entries are binding, "Proposed" entries are recommendations that still need confirmation.

Related documents:

- [ROADMAP.md](ROADMAP.md): scope, stages, milestones, and acceptance criteria.
- [MIGRATION_SPEC.md](MIGRATION_SPEC.md): the first migration path, Next.js to Astro + Preact.
- [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md): status model, metrics, and report formats.
- [CLI.md](CLI.md): the implemented help and version shell and remaining planned commands.
- [ENGINEERING.md](ENGINEERING.md): implementation language, dependency policy, and engineering practices.
- [SECURITY.md](SECURITY.md): threat model and safety controls.

## 1. Purpose

This document defines the conceptual architecture of the migration engine: the pipeline, subsystem boundaries, core data structures, rule engine, planning and execution model, verification, and reporting. Later implementation work must follow it or change it explicitly through a documented decision.

## 2. Architectural drivers

When these drivers conflict, the earlier one wins.

1. **Safety.** Source repositories are untrusted input. The engine never modifies them, never executes their code without explicit authorization, and never writes outside the designated output location.
2. **Correctness and honesty.** The engine never presents an incomplete, unverified, or unknown result as successful. Anything that is not handled by a rule is reported as unknown or unsupported ("fail closed", `D-009`).
3. **Determinism.** The same input snapshot, options, engine version, and ruleset version produce byte-identical plans, output files, and reports. The only exception is a clearly separated run-metadata section (timestamps, durations).
4. **Explainability.** Every classification, operation, and generated output region can be traced to a rule, a source location, and a documented reason.
5. **Maintainability and architectural preparation.** Understandable structures (KISS), with mandatory consideration of the documented V1 to V3 evolution, extensibility, and compatibility (`D-033`). Record design rationale and distinguish future contracts from implemented behavior.
6. **Performance.** Analysis of typical projects should feel instantaneous, and large repositories must stay tractable. External verification (installing packages, running builds) will dominate end-to-end time. The engine's own work must not.
7. **Extensibility.** New source frameworks, target frameworks, and rules must be addable without rewriting the core. Abstractions are introduced only when a second concrete use exists.

## 3. System context

```mermaid
flowchart LR
  user[Developer or CI job] -->|arguments| cli[frameshift CLI]
  cli -->|engine API| engine[Migration engine]
  engine -->|read only| src[(Source project, untrusted)]
  engine -->|staged writes| out[(Output project)]
  engine -->|documents| docs[(Analysis, plan, report)]
  engine -. explicit authorization only .-> tools[External toolchain: package manager, astro]
  tools -. temporary copy .-> out
  future[Future GUI or integrations] -. same engine API or JSON contracts .-> engine
```

- The CLI is a thin adapter. All migration logic lives in the engine (`D-005`).
- The engine does not access the network during inspection, analysis, planning, or migration (`D-006`).
- External tools run only for build, static-output, and runtime verification, only with explicit authorization, and only on temporary copies (see [SECURITY.md](SECURITY.md)).

## 4. Pipeline

### 4.1 Evaluation of the initial pipeline

The initial proposal was:

Repository Discovery → Parsing → Static Analysis → Dependency Graph → Intermediate Representation → Compatibility Analysis → Migration Planning → Rule-Based Transformation → Target Generation → Verification → Reporting

The order is sound, but the proposal needs six refinements:

1. **Reporting is cross-cutting, not a final stage.** Every stage produces evidence: findings, decisions, operations, and verification results. These go into an append-only *evidence ledger*, and reports are projections of it. A failure in stage N still produces a meaningful report about stages 1 to N.
2. **A snapshot step is needed after discovery.** All later stages work on an immutable, content-hashed snapshot of the source files. This guarantees a consistent view even if files change during a run. It also gives plans and reports a verifiable input identity.
3. **"Dependency graph" means several graphs.** These are the package graph (manifest and lockfile), the module graph (imports and exports), the component graph (which component renders which), and the route tree. All of them belong to semantic analysis, not to a separate stage.
4. **Phases need hard side-effect boundaries.** Analysis is read-only, planning is a pure function, execution writes only to a staging area, and verification may run external tools only when authorized. These boundaries make dry-runs trivial, and they make the plan inspectable before anything is written.
5. **Transformation and target generation run together, per unit, before a separate commit.** Generated files go to a staging directory first. A commit step then moves the result into place atomically.
6. **Verification has levels and never feeds back automatically.** The engine does not loop "transform, build, adjust" until something compiles. Such loops hide problems and destroy determinism. Verification results are reported, and humans or later deterministic rules act on them.

### 4.2 Refined pipeline

```mermaid
flowchart TB
  subgraph AN[Analyze: read only]
    D[Discover] --> S[Snapshot] --> P[Parse] --> SA[Semantic analysis] --> M[Build project model] --> C[Assess compatibility]
  end
  subgraph PL[Plan: pure function]
    MAP[Map output paths] --> SEL[Select rules] --> OPS[Generate and order operations] --> PLAN[(Plan)]
  end
  subgraph EX[Execute: staging only]
    T[Transform and generate] --> ST[(Staging directory)] --> CM[Commit]
  end
  subgraph VE[Verify: optional levels]
    V1[Structural] --> V2[Build] --> V3[Static output]
  end
  AN --> PL --> EX --> VE
  LED[(Evidence ledger)]
  AN -.-> LED
  PL -.-> LED
  EX -.-> LED
  VE -.-> LED
  LED --> REP[Report projections]
```

| Phase | Input | Output | Side effects | Planned command |
| --- | --- | --- | --- | --- |
| Inspect | Source path | Project identity summary | Reads a bounded set of files | `frameshift inspect` (implemented, `D-075`) |
| Analyze | Source path, target | Project model, assessment, findings | Reads source files | `frameshift analyze` |
| Plan | Analysis, target profile, ruleset | Plan with ordered operations | None | `frameshift plan` |
| Execute | Plan, output location | Output project, manifest | Writes to staging, then commits | `frameshift migrate` |
| Verify | Output project, optional source | Verification evidence | Structural: none. Build, static output, and runtime: runs external tools on temporary copies, only when authorized | `frameshift verify` |
| Report | Ledger or stored report | Human-readable or JSON reports | None | `frameshift report` |

## 5. Core concepts

| Term | Meaning |
| --- | --- |
| Source project | The project being migrated. Always treated as untrusted and read-only. |
| Output project | The newly generated project in a separate directory. |
| Snapshot | Immutable, content-addressed set of source files used for one run. Its hash identifies the input. |
| Unit | The smallest independently reported element of a migration, such as a route, layout, component, module, stylesheet, or configuration concern. Units do not overlap. The full taxonomy is in [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md). |
| Inventory item | A counted but unweighted element: dependency, verbatim file, out-of-scope file, or ignored path. |
| Feature | A catalogued framework construct, such as `nextjs.api.link` or `react.hook.use-state`, identified by a stable feature ID. |
| Feature occurrence | One detected instance of a feature in a unit, with source location and extracted facts. |
| Finding | A diagnostic: code, severity, location, explanation, remediation, and the rule or check that produced it. |
| Assessment status | Pre-execution classification of a feature occurrence or unit: `auto`, `review`, `unsupported`, or `unknown`. |
| Rule | A versioned, tested transformation or assessment unit with declared preconditions, outputs, and verification requirements. |
| Ruleset | All rules, knowledge base data, and target profiles shipped with one engine version. |
| Operation | One planned output action: create, transform, copy, or preserve a file, or generate a configuration file. |
| Plan | The ordered, hashed list of operations and manual tasks, bound to a snapshot and ruleset. |
| Manifest | Provenance record written into the output project: versions, plan hash, output file hashes, and their origins. |
| Evidence ledger | Append-only record of findings, decisions, operations, and verification results for a run. |
| Target profile | Pinned target framework versions and conventions, for example `astro7-preact10`. |
| Knowledge base | Embedded, versioned data about framework APIs, package compatibility, and target capabilities. |

## 6. Subsystems and boundaries

The engine is layered. Dependencies point downward only. The CLI and future frontends sit on top and use the engine only through its public interface.

```mermaid
flowchart TB
  IF[Interfaces: engine API facade, CLI adapter]
  RP[Reporting: projections, JSON documents, renderers]
  AS[Assurance: verification runner, output comparison]
  OU[Output: edit engine, printers, executor, manifest]
  MG[Migration: assessment, rules, planner]
  MK[Model and knowledge: project model, knowledge base, target profiles]
  SE[Semantics: module resolution, scopes, graphs, static evaluator, framework fact extraction]
  SY[Syntax: lexers, parsers, syntax trees, position tables]
  IN[Input: discovery, snapshot store]
  FD[Foundation: diagnostics, identifiers, paths, hashing, limits, deterministic collections]
  IF --> RP --> AS --> OU --> MG --> MK --> SE --> SY --> IN --> FD
```

The vertical chain shows layer order, not single-step dependencies. A layer may use any layer below it.

Boundary rules:

- **Foundation** has no knowledge of frameworks. Its full scope is diagnostics, stable identifiers, the internal path model (canonical, slash-separated, root-relative), hashing, resource limits, and collections with deterministic iteration. The text, path, stable catalog identifiers, unit identity values, bounded finding records with deterministic report-local numbering, bounded SHA-256 streaming and canonical digest values (`D-058`, `D-059`, IMPLEMENTATION.md sections 18 and 19), dense key tables with slot-addressed results (`D-060`, section 20), and run limits with atomic budgets (`D-061`, section 21) are implemented, so every foundation contract exists. The engine's `limits` module bundles all subsystem limit types, checks their relations, and lists their values canonically. Finding scopes, limits, and complete ordering are in IMPLEMENTATION.md section 17 (`D-057`); the evidence ledger and report projections remain future work. Catalog identifier syntax and limits are in section 15 of [IMPLEMENTATION.md](IMPLEMENTATION.md) (`D-055`); membership and shipped catalogs remain future work. Unit identity builders distinguish file, URL-pattern, and config subjects with injective percent encoding (`D-056`, IMPLEMENTATION.md section 16); building units from discovered files and model entities remains planned.
- The implemented path values are `SourcePath` and the stronger `OutputPath` (`D-053`, `D-054`). They require canonical slash-separated root-relative UTF-8, reject ambiguous forms without normalization, and preserve source spelling. Their complete value contract and filesystem limits are in section 14 of [IMPLEMENTATION.md](IMPLEMENTATION.md); native conversion, collision detection, and safe writes remain future subsystem work.
- The SHA-256 foundation hashes exact bytes without I/O or third-party API leakage. It does not define a snapshot or plan hash format itself. The snapshot identity defines its own versioned, length-framed record stream with a domain tag (`D-067`, section 24 of [IMPLEMENTATION.md](IMPLEMENTATION.md)); the future plan format must do the same. Plain concatenation of fields is insufficient.
- **Input** is the only layer that touches the source filesystem. Every later layer reads the snapshot. The crate `frameshift-input` implements discovery: it lists directories, reads link targets, and opens only `.gitignore` files, through `std::fs` (`D-064`, section 22 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). Its snapshot store reads each admitted file on demand and at most once, hashes it, retains the bytes for the run, and stops the run when a file changed after listing (`D-067`, section 24).
- **Syntax** knows file formats but not frameworks.
- **Semantics** knows language semantics (modules, scopes) in general. Framework knowledge enters through *source adapters*, which extract framework facts. The crate `frameshift-package` implements the package part of this layer without framework knowledge: npm versions and ranges, the `package.json` model, package manager detection, npm, Bun, pnpm 9.0, and Yarn classic and Berry lockfiles, and workspace members with package selection, read through the snapshot store (`D-069`, `D-070`, `D-072`, `D-073`, sections 26, 29, and 30 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). The first source adapter, `frameshift-nextjs`, detects Next.js projects, versions, configuration files, and routers on top of it (`D-074`, section 31).
- **Model and knowledge** hold data and contain no I/O.
- **Migration** computes. Planning is a pure function of the model, options, and ruleset.
- **Output** is the only layer that writes files, and only to staging and output locations given by the planner and executor.
- **Assurance** is the only layer allowed to start external processes, and only behind an explicit authorization flag.
- **Reporting** turns the ledger into documents. Terminal formatting belongs to the CLI, not to the engine. The engine's deterministic JSON document writer and the `frameshift.inspection` document exist (`D-075`, section 32 of [IMPLEMENTATION.md](IMPLEMENTATION.md)); the ledger and the migration documents remain planned.

## 7. Project discovery

Discovery determines what the project is before deep analysis starts.

Items 1 to 3 are implemented by the discovery walker (`D-064`, `D-065`, sections 22 and 23 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). The package model (`D-069`, `D-070`, `D-072`, sections 26 and 29) implements item 7 and reports package files that are not UTF-8. Workspace detection (`D-073`, section 30) implements item 8: it evaluates `workspaces` and `pnpm-workspace.yaml` with the pattern semantics of each modeled package manager release and resolves the selected package through the root lockfile. The Next.js source adapter (`D-074`, section 31) implements items 5 and 6 for Next.js: combined evidence, the configuration file that the version loads, the router directories, and versions from the root lockfile or the lower bound of the declared range, marked imprecise. Encoding classification of other files remains planned.

1. **Root and boundaries.** The root is the directory the user passes. Discovery never leaves it and follows no link: symbolic links, junctions, and every other Windows reparse point are links in this sense. A link whose target resolves outside the root is excluded and reported (`FS1003`). A link whose target resolves inside the root through traversed real directories is recorded as a reference (`FS1005`), so its target is processed once, at its own path. Device files, FIFOs, and sockets are skipped (`FS1002`). Names that the canonical path grammar cannot represent are counted and reported at their parent directory (`FS1001`). Siblings are visited by name bytes with a slash appended to directories, which is byte-wise path order, so results never depend on the operating system's enumeration order.
2. **Ignore policy.** Always ignored, at every depth and ASCII-case-insensitively: version control entries (`.git`, `.hg`, `.svn`), `node_modules`, framework build output (`.next`; the static export directory follows once configuration is evaluated), and environment files (`.env`, `.env.*`, except names ending in `.example`, `.sample`, or `.template`), which are never even opened (see [SECURITY.md](SECURITY.md)). Patterns from the project's own `.gitignore` files inside the root are honored with Git's semantics, byte-wise and case-sensitively. The user's global Git configuration, `.git/info/exclude`, and files above the root are never consulted, because that would make results depend on the machine. Every ignored path is listed with its reason; nothing below an ignored directory is read.
3. **Resource limits.** Per-file size, total file count, total bytes, directory depth, entries read, `.gitignore` bytes, and matching work have deterministic limits (`D-061`, `D-064`, `D-065`; defaults in section 21 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). A file over the per-file limit is accounted for and reported (`FS1004`), and becomes an `unknown` unit once units exist; a directory beyond the depth limit is not traversed (`FS1009`). The first file, directory, or `.gitignore` file in canonical order that would exceed a cumulative limit stops discovery with a finding, and the inventory is marked incomplete. Nothing is silently skipped. Limits are size- and count-based, not time-based, so results do not depend on machine speed.
4. **Encoding.** UTF-8 is expected, and a UTF-8 byte order mark is recognized and preserved. Other text encodings make the file `unknown`. Binary files are identified by extension and content sniffing, and are treated as assets.
5. **Framework signatures.** A framework is identified by combined evidence: dependency declarations (`next` in `package.json`), configuration files (`next.config.js`, `.mjs`, `.ts`, found as regular files as Next.js finds them, `D-076`), directory conventions (`app/`, `src/app/`, `pages/`, `src/pages/`), and lockfile entries. A single signal is never sufficient on its own.
6. **Version detection.** Resolved versions come from the lockfile first. If there is no usable lockfile, the declared range in `package.json` gives a lower bound and the result is marked imprecise. Version-dependent rules then behave conservatively. `node_modules` is not read.
7. **Package manager.** The `packageManager` field in `package.json` takes precedence. Otherwise the lockfile type decides: `package-lock.json` (npm), `pnpm-lock.yaml` (pnpm), `yarn.lock` (Yarn classic or Berry, told apart by format), or `bun.lock` (Bun text lockfile). The binary `bun.lockb` format is detected but not parsed.
8. **Workspaces.** Workspace declarations (`workspaces` in `package.json`, `pnpm-workspace.yaml`) are evaluated as the package manager release that installs the project reads them; where that release is not determined, a directory is a member only when every candidate release selects it. The run's root is the workspace root, and nothing above it is read. V1 migrates one package directory at a time. Sibling workspace packages are treated as external dependencies with unknown compatibility.

## 8. Parsing and syntax representation

### 8.1 Why structural parsing

Textual replacement (regular expressions, string search) is excluded for transformations. It cannot tell code from strings, comments, or JSX text. It cannot respect scopes: `Link` may be an aliased import, or a local variable that shadows one. And it cannot guarantee syntactically valid output. Every transformation in Frameshift operates on parsed structure.

### 8.2 Formats

| Format | Purpose | Notes |
| --- | --- | --- |
| JavaScript, TypeScript, JSX, TSX (`.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts`, `.tsx`) | Source code analysis and transformation | Full syntax coverage required. Types are parsed completely into structured nodes with exact spans and preserved without type checking (`D-042`). Declaration files (`.d.ts`, `.d.mts`, `.d.cts`) use an ambient dialect that the caller selects (`D-043`). |
| CSS, SCSS, indented Sass | Imports, `url()` references, CSS Modules class names, Tailwind directives | Tokenizer following the CSS Syntax Level 3 Editor's Draft at a pinned revision, implemented (`D-077`, IMPLEMENTATION.md section 33). A generic structural parser with nesting and bounded recovery is implemented (`D-078`, IMPLEMENTATION.md section 34). Selector, value, and at-rule grammar validation remains later analysis. SCSS and indented Sass statements, SassScript, and structured style rule selectors are implemented under `D-079` and `D-081`; full Sass conformance remains M3 work, with explicit unchecked grammar in the current tree (IMPLEMENTATION.md section 35). |
| JSON, JSONC | `package.json`, `tsconfig.json`, `jsconfig.json`, npm and Bun lockfiles | JSONC (comments, trailing commas) is required for TypeScript configuration and `bun.lock`. |
| YAML 1.2 | `pnpm-lock.yaml`, `pnpm-workspace.yaml`, Yarn Berry lockfiles | Full syntax implemented (`D-071`, IMPLEMENTATION.md section 27), with the pnpm lockfile, `pnpm-workspace.yaml` (`D-073`, section 30), and Berry lockfile schemas. Flat references preserve aliases without expansion; tags never instantiate objects. Consumers validate their own schemas. |
| Yarn classic lockfile | `yarn.lock` v1 | Dedicated format, implemented as Yarn 1.22.22 reads it (`D-072`, IMPLEMENTATION.md section 29). Input that Yarn would read differently or truncate is rejected. |
| Astro templates | Re-parsing generated output for structural verification | Frontmatter is parsed as TypeScript. The template parser is scoped to what the printer generates and is refined in M7 (see [ROADMAP.md](ROADMAP.md)). |
| HTML | Static output comparison during verification | Build-versus-adopt decision pending (`Q-005`). |

The implementation strategy for these parsers (purpose-built or adopted) is covered in [ENGINEERING.md](ENGINEERING.md) and `P-002`.

### 8.3 Syntax tree requirements

- **Precise spans.** The M0 foundation already provides validated source spans and indexed positions, and the expression arena records node byte spans in the original text (see [IMPLEMENTATION.md](IMPLEMENTATION.md)). The complete M0 per-file syntax trees use the same representation. Line and column positions come from a per-file line table. Reports use byte offsets as the canonical location, with derived line and column for humans (columns count Unicode scalar values, `D-029`).
- **Comments and trivia.** Comments are kept with their spans in a per-file list. Attaching comments to nodes is a separate, deterministic step. The JavaScript tree omits whitespace; CSS component values retain whitespace tokens as CSS Syntax requires (`D-077`, `D-078`). The original text is always available, and unchanged regions are copied byte for byte.
- **AST, not CST, initially.** A span-precise abstract syntax tree combined with the original text supports edit-based rewriting (section 14) without a full concrete syntax tree. Experiment E2 confirmed this assumption, and `P-003` is accepted (`D-049`): the per-file comment list and node spans account for every comment, so a printer can prove that none is dropped. A lossless concrete syntax tree remains the fallback if later evidence contradicts it.
- **Error policy (`D-034`).** A file with syntax errors becomes `unknown` and is never transformed, including recovered portions. The future complete parser reports multiple errors through bounded recovery only at safe grammar boundaries. Diagnostic and recovery-work limits are explicit; every recovery step must make progress. If lexical context cannot be recovered safely, parsing stops with a diagnostic. Recovery never guesses repairs. CSS synchronizes at whole component values, semicolons, enclosing block boundaries, and EOF, with explicit depth, node, diagnostic, and work limits (`D-078`). Its diagnostics prohibit transformation of the whole file. JavaScript recovery remains M3 work; its scanners and parser return one error without partial results.
- **Sass (`D-079`, `D-081`).** The two explicit surface syntax modes share a flat arena with original spans and comments, without source rewriting, evaluation, or module loading. Style rule selectors are structured with the selector and keyframe grammars that Dart Sass applies during evaluation; evaluation-dependent selectors carry evidence. The parser exposes unchecked grammar as evidence; no current Sass result authorizes transformations. IMPLEMENTATION.md section 35 records coverage and the full-conformance work still needed for M3.
- **Resource safety.** Nesting depth and token counts are bounded, so hostile input cannot exhaust the stack or memory (see [SECURITY.md](SECURITY.md)).

## 9. Semantic analysis

Semantic analysis turns syntax trees into facts.

- **Module resolution.** Covers relative specifiers with extension and index resolution, the TypeScript `baseUrl` and `paths` settings from `tsconfig.json` or `jsconfig.json`, and package `imports` fields. Bare specifiers are mapped to dependencies by package name; package contents are never resolved. Asset and stylesheet imports are classified by extension. A dynamic `import()` with a literal specifier is resolved. A computed specifier is an `unknown` feature occurrence.
- **Scopes and bindings.** Declarations, imports, and shadowing are tracked, so that each reference resolves to its binding. This is how the engine knows that `<L href="/">` refers to `next/link` after `import L from 'next/link'`, and that a local variable named `Link` does not.
- **Graphs.**
  - *Package graph:* declared dependencies, resolved versions, and whether the code uses each one.
  - *Module graph:* imports and exports, including re-exports and barrel files.
  - *Component graph:* each JSX element resolved to its component definition where possible.
  - *Route tree:* segments, parameters, layout chains, and special route files.
- **Render contexts.** Next.js decides where code runs from `'use client'` boundaries: a module reachable through imports from a client boundary runs in the client context. Analysis computes the set of contexts (server, client, or both) for every component and module. This set decides the target strategy (see [MIGRATION_SPEC.md](MIGRATION_SPEC.md)).
- **Environment classification.** Code is classified as build-time only, browser only, or shared. This matters for environment variables, Node.js API usage, and browser API usage.
- **Static evaluator.** Configuration files (`next.config.*`, `tailwind.config.*`, `postcss.config.*`) and some declarations (metadata objects, `generateStaticParams` results) are evaluated by a partial evaluator over a safe subset. That subset covers literals, object and array literals, template literals with known parts, `const` bindings, and constant imports from local modules. Every other expression evaluates to `Unknown(reason)`. The evaluator never executes project code.
- **Framework fact extraction.** A source adapter (for V1, the Next.js adapter) extracts framework-specific facts. Examples are route segment configuration exports, metadata, directives, and calls to framework APIs.

## 10. Project model and intermediate representation

### 10.1 MVP: a project model, not a universal IR

V1 uses a project model that is just large enough for the Next.js to Astro + Preact path (`P-004`). It has two layers:

1. **Syntax layer:** per-file syntax trees with spans, comments, and original text. Function bodies, expressions, and types stay here.
2. **Semantic layer:** units and their relationships.

```mermaid
erDiagram
  PROJECT ||--o{ FILE : contains
  PROJECT ||--o{ DEPENDENCY : declares
  FILE ||--o{ UNIT : defines
  UNIT ||--o{ FEATURE_OCCURRENCE : contains
  UNIT ||--o{ FINDING : has
  ROUTE ||--|| UNIT : "is a"
  ROUTE }o--o{ LAYOUT : "wrapped by (ordered chain)"
  COMPONENT ||--|| UNIT : "is a"
  COMPONENT }o--o{ COMPONENT : renders
  UNIT }o--o{ UNIT : imports
  UNIT }o--o{ DEPENDENCY : uses
  UNIT }o--o{ ASSET : references
```

Model entities name concepts in framework-neutral terms where the concept is genuinely shared: route, layout, component, module, stylesheet, asset, configuration concern, dependency, render context, and hydration need. Information that only makes sense for one framework is stored in namespaced *facets*. For example, `nextjs.segmentConfig` stores `dynamic`, `revalidate`, and `dynamicParams`. Target-specific planning data never enters the source model.

### 10.2 Why not a universal IR first

- The V1 model uses neutral concepts and namespaced framework facets, with documented extension boundaries for V2. A universal IR requires evidence that its semantics preserve the execution models of additional frameworks (`P-004`, `D-033`).
- Frameworks differ in execution model: React re-rendering, Astro build-time rendering, Svelte compile-time reactivity, Vue templates. No finite IR can represent all of them faithfully. A representation that pretends to would quietly lose information.
- The migration value is concentrated in architectural structure (routes, boundaries, rendering modes, assets, configuration). Arbitrary program semantics stay in syntax trees and are transformed by code-level rules.

### 10.3 Evolution in V2

In V2 the model is generalized into an intermediate representation with three parts:

- **Common core:** concepts that exist in at least two supported frameworks with compatible semantics, such as routes with parameters, layouts, components with props, static assets, stylesheets, and dependencies.
- **Facets:** framework-specific information attached to core entities and preserved without forcing it into the core.
- **Pair-specific recipes:** migration rules that need the facets of one source and one target. They coexist with generic recipes that rely only on the core.

A concept moves into the core only when two adapters need it with compatible meaning. The V2 refactoring is proven by unchanged golden outputs for all V1 fixtures (see [ROADMAP.md](ROADMAP.md)).

## 11. Compatibility assessment

1. **Feature catalog.** Every supported or recognized construct has a stable feature ID. The catalog for V1 is the compatibility matrix in [MIGRATION_SPEC.md](MIGRATION_SPEC.md).
2. **Detection.** Detectors produce feature occurrences from model facts. Constructs that analysis cannot match to the catalog become occurrences of a generic `unknown` feature with their location.
3. **Assessment rules.** Each occurrence receives `auto`, `review`, `unsupported`, or `unknown`, with a reason. The status may depend on context: source version, render context, target profile, or knowledge base data.
4. **Unit status.** A unit's status is the most severe status among its occurrences, ordered `auto` < `review` < `unknown` < `unsupported`. All occurrences remain listed.
5. **Transitive readiness.** Unit status is local. Route readiness looks at the route's transitive dependency closure: layouts, rendered components, imported modules, and stylesheets. This gives route-level answers such as "this route is blocked by a context provider shared across islands" (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)).
6. **Dependencies.** The knowledge base assigns each declared package a compatibility class for specific version ranges. For the first migration path, the classes are `framework`, `compatible`, `compat` (works through `preact/compat`), `compat-unverified`, `incompatible`, and `unknown` (see section 14 of [MIGRATION_SPEC.md](MIGRATION_SPEC.md)). Code that uses an incompatible package gets an occurrence of `dependency.compatibility.incompatible`. An unused declared package affects nothing except the dependency inventory.
7. **Project-level verdicts.** Some answers concern the whole project, such as "can the result be built as a static site". These verdicts are derived from explicit facts (for example, "request-time APIs are used on 9 of 12 routes"), not from percentage thresholds.

### 11.1 Knowledge base

The knowledge base is data embedded in the binary and versioned with each release. It contains:

- the framework API catalog, such as Next.js APIs, the versions that introduced or removed them, and their migration strategy;
- package compatibility entries, each with a version range, status, notes, and the fixture IDs that provide evidence;
- the `preact/compat` coverage table for the pinned Preact version;
- target profiles: pinned versions of Astro, `@astrojs/preact`, and Preact, plus required Node.js versions and conventions.

Every entry records the evidence and the version it was last verified against. Changes go through pull requests with tests, like code. The knowledge base version appears in every plan and report.

## 12. Rule engine

### 12.1 Rule anatomy

A rule is code plus declarative metadata. Illustrative pseudocode, not a committed syntax:

```text
rule nextjs-to-astro-preact/route.static-page
  version: 3
  source: nextjs >= 14.0 < 17, router = app
  target: profile astro7-preact10
  matches: feature nextjs.app.page
  requires:
    - unit.kind == route
    - every route segment is static
    - the page component is convertible to an Astro component
  produces:
    - create src/pages/<route path>.astro from the page component
  excludes: route.dynamic-static-params, route.catch-all
  verifies:
    - structural: output re-parses, imports resolve, no residual next/* imports
    - static-output: route compared with the source build
  documentation: explanation, examples, fixtures
```

### 12.2 Lifecycle

| Aspect | Design |
| --- | --- |
| Registration | Rules are registered at compile time in a registry per migration path. IDs are unique. Metadata is validated by tests. There is no runtime loading in V1 or V2 (plugins are deferred, `DF-002`). |
| Matching | A feature ID index finds candidate rules. Preconditions are predicates over model facts and are evaluated without side effects. |
| Prioritization | V1 has no numeric priorities. For any feature occurrence, at most one transformation rule may match: preconditions must be mutually exclusive. Tests check this, and the planner checks it again. Ambiguity produces finding `FS4001`, and the occurrence becomes `unknown` (fail closed). V2 introduces explicit composition and priorities when declarative recipes require them. |
| Validation | Preconditions are checked before planning. Postconditions are checked after emission: output parses, references resolve, residue checks pass. A postcondition failure marks the unit `failed`. |
| Composition | Operations from different rules must target disjoint output files, or disjoint spans within one file. Overlapping edits are detected at plan time and handled like ambiguity. |
| Execution | Rules never write files. They return operations, and the executor applies them. |
| Auditing | Each operation records the rule ID and version, the source spans it consumed, and the output regions it produced. These records form the provenance in the manifest. |
| Testing | Each rule has positive fixtures, negative fixtures (preconditions not met), and golden outputs. Rule documentation is the source for `frameshift explain`. |
| Versioning | A rule's version increases whenever its output can change. The ruleset version combines the engine release, knowledge base version, and target profile. Plans record all of them. |

### 12.3 Rule identifiers

A full rule ID is `<migration-path-id>/<area>.<name>`, for example `nextjs-to-astro-preact/route.static-page`. Reports for a single migration path may show the short form `route.static-page`. Finding codes and the full naming scheme are defined in [ENGINEERING.md](ENGINEERING.md) and [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).

## 13. Migration planning

The planner knows everything it intends to do before any file is written.

1. **Path mapping (pass 1).** Every unit and copied file receives its output path. This is a global step, because import rewriting needs the final location of every module. Path collisions, including collisions on case-insensitive filesystems, are detected here.
2. **Operation generation (pass 2).** The selected rule for each unit produces operations. Units classified `unsupported` or `unknown` produce *preserve* operations and manual tasks instead.
3. **Ordering.** Operations are sorted by output path, then operation ID. They write distinct files, so they can run in parallel without affecting the result.
4. **Binding.** The plan records the snapshot hash, engine version, ruleset version, target profile, and every option that affects output. The plan hash covers all of them.
5. **Plan file as an approval artifact.** `frameshift plan --out plan.json` writes the plan for inspection. `frameshift migrate --plan plan.json` does not execute the file's contents. It recomputes the plan from the source and refuses to continue if the hashes differ (`P-005`). Tampered or stale plan files therefore cannot inject operations, and the approved plan is exactly what runs.

Operation kinds:

| Kind | Meaning |
| --- | --- |
| `create` | Generate a new file, such as an `.astro` component, from model data and source slices. |
| `transform` | Apply span edits to a source file and write the result to its output path. |
| `copy` | Copy a file verbatim. |
| `preserve` | Copy an unmigrated source file into `.frameshift/unmigrated/` for manual work. |
| `config` | Generate a structured configuration file (`package.json`, `astro.config.mjs`, `tsconfig.json`). |

Manual tasks are part of the plan but are not operations. Examples: "rename environment variables in your hosting provider" and "port the server action in `app/contact/actions.ts`".

## 14. Transformation and code generation

### 14.1 Two levels

- **Structural transformations** are driven by the project model: file layout, routes, layout chains, configuration, and dependencies.
- **Code-level transformations** are driven by syntax trees: JSX conversion, import rewriting, API replacement, and directive removal.

### 14.2 Edit engine

The M0 edit core implements span edits against immutable original text: replace, insert, or delete a byte range. Edits must not overlap. Insertions at the same offset use operation order, then edit order; each ordering pair is unique. Insertions at a replacement boundary precede a replacement starting there and follow one ending there. Insertions inside a replacement are conflicts. Invalid edits return errors without partial output. Text outside edited spans is copied byte for byte. The core also records text-region provenance and deleted spans. Exact contracts and limits are in [IMPLEMENTATION.md](IMPLEMENTATION.md). Parser-driven edits exist in the E2 driver, and the output layer's emitted text extends provenance across several source files; rule identifiers in provenance and the manifest remain planned. E2 validated the approach (`P-003`, `D-049`).

### 14.3 Printers

New files are produced by printers, for Astro components, JSON, and JavaScript configuration modules. The Astro component printer and the conversion of React JSX into its template model exist in `frameshift-emit` ([IMPLEMENTATION.md](IMPLEMENTATION.md), section 10); the JSON and configuration printers are planned. Printers build generated structure canonically and embed slices of original source text for expressions, statements, and types where possible. When a page component becomes an `.astro` file:

- Its body statements go to the frontmatter as source slices, with span edits applied.
- Its JSX becomes the template. The printer walks the JSX tree, prints markup canonically, and copies expression containers as source slices.

Printer rules:

- **Escaping.** Every string that comes from the source and is embedded in a new syntactic context (HTML text, attributes, JavaScript string literals, template expressions) is escaped for that context. Source content can never inject markup or code into generated files (see [SECURITY.md](SECURITY.md)).
- **Imports.** Imports are added, removed, and rewritten with deduplication. Retained imports keep their order. Generated imports follow them in a deterministic order.
- **Identifiers.** Generated identifiers are checked against the bindings in scope. Collisions are resolved by deterministic suffixing.
- **Formatting.** Generated code uses the project's dominant indentation style, detected from the snapshot, and falls back to two spaces. External formatters are not run, because that would require executing third-party tools.
- **Comments.** Comments in copied regions are preserved. Leading comments of a removed construct stay at the removal position.
- **Line endings and encoding.** Transformed files keep their original line endings and byte order mark. New files use the project's dominant line ending, also detected from the snapshot. Operating system defaults are never used.

### 14.4 Output layout and generated support files

- **Layout.** The output follows the conventional target structure, for Astro with all code under `src/`. Data read at build time stays where its filesystem paths expect it (`D-024`). Details are in [MIGRATION_SPEC.md](MIGRATION_SPEC.md).
- **Compatibility helpers.** When a rule needs runtime support code (for example, a small navigation shim), the code is generated into `src/frameshift-compat/`. Each helper is listed in the report with its purpose and removal guidance.
- **Unmigrated sources.** Files that cannot be migrated are copied into `.frameshift/unmigrated/` under their original relative paths. Nothing is dropped silently. Every source file is transformed, copied, preserved, or reported as out of scope or ignored.
- **Manifest and report.** `.frameshift/manifest.json` and `.frameshift/report.json` record provenance and results. `.frameshift/SUMMARY.md` summarizes remaining manual tasks for humans, and the CLI points to it after every migration (`P-011`).

## 15. Safe execution

| Requirement | Design |
| --- | --- |
| Dry-run | `migrate --dry-run` runs everything except the commit. It shows the files it would write and the summary, and writes nothing unless a report file is requested. |
| Non-destructive output | The output location must not exist, or must be empty, unless an explicit overwrite policy applies. It must not be inside the source directory, the source must not be inside it, and it must not be a filesystem root or the user's home directory. |
| Staging and atomic commit | All files are written to a staging directory next to the output location, on the same filesystem. On success, the staging directory is renamed to the output path, which is atomic when the target does not exist. |
| Overwrite policies | Default: fail if the output exists. `replace-managed` replaces an existing output only if its manifest identifies it as Frameshift output and every file still matches the manifest hashes, so no manual edits can be lost. The previous output is moved aside, never deleted. Arbitrary directories are never deleted. |
| Recovery from partial failures | A crash leaves the source untouched, the output absent or unchanged, and a marked staging directory. The next run refuses to proceed until that staging directory is cleaned up. Cleanup removes only directories that carry the Frameshift staging marker. |
| Unit-level failures | A failing operation marks its unit `failed`, and the failure is recorded. Other units continue, and the output is committed with the failures reported. In `--strict` mode, any failure aborts the run before commit. |
| Idempotency | Re-running with the same inputs produces a byte-identical output tree. If the existing managed output is already identical, the commit is skipped. |
| Determinism | No timestamps, random values, absolute paths, or machine-specific data in output files, plans, or manifests. |
| Isolation | Temporary files live inside the staging directory. The executor accepts only validated relative output paths, so a path can never resolve outside the output location (see [SECURITY.md](SECURITY.md)). |

## 16. Verification

Verification produces evidence, never proof. Levels are cumulative:

| Level | Checks | Requires | Demonstrates | Does not demonstrate |
| --- | --- | --- | --- | --- |
| `structural` | Generated files re-parse. Imports resolve within the output. No residual source-framework APIs (such as `next/*` imports or `'use client'` directives) remain unless flagged. Every source route maps to an output route or a finding. Manifest hashes match. | Nothing external | The output is syntactically valid and internally consistent | That it builds, runs, or behaves the same |
| `build` | Install dependencies and run the target's type check and production build | `--allow-exec`; network access for installation | The output compiles with the pinned toolchain | Behavioral equivalence |
| `static-output` | Build source and output and compare the generated HTML per route: route set, normalized text content, links, headings, landmarks, metadata, image references | `--allow-exec`; a statically exportable source | That build-time rendered content matches on the compared dimensions | Runtime interactivity, visual layout, CSS cascade, timing |
| `runtime` (V1, M10) | Headless browser checks of hydration, declared scripted interactions, and selected accessibility-tree comparisons | User-installed external browser; `--allow-exec`; a buildable, statically exportable source | Selected runtime behaviors on the routes and checks exercised | Full behavioral equivalence, untested interactions, visual layout, or all timing behavior |

Rules declare which levels can verify their outputs. Evidence is recorded per unit: a unit counts as verified at a level only if every check covering it ran and passed. The external tool runner works on a temporary copy of the output with a scrubbed environment, enforces timeouts, limits captured output, and treats tool output as untrusted data (see [SECURITY.md](SECURITY.md) and [TESTING.md](TESTING.md)).

Runtime verification is required V1 scope (`P-013`), with implementation in M10 after executable migration fixtures exist. Browser selection, the versioned interaction-check protocol, and handling of dynamic browser observations must be decided in M10 before implementation. Missing checks or browser prerequisites cannot count as passed. V2 M23 extends this mechanism across adapters and targets.

## 17. Reporting

The evidence ledger holds four entry types: findings, decisions (which rule was chosen, or why none matched), operations, and verification results. Reports are deterministic projections of the ledger. Supported projections: project summary, per-file, per-unit, per-route, dependencies, configuration changes, rules applied, findings, verification, and manual tasks. Machine-readable documents are versioned JSON. Human-readable rendering happens in the CLI. Metrics are defined precisely in [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).

## 18. Interfaces

- **Engine API.** A small in-process interface: `inspect`, `analyze`, `plan`, `execute`, `verify`, and `render`. Each takes explicit options and returns structured results. `inspect` exists as `frameshift_engine::inspect::inspect`, whose result renders its own document (`D-075`); the others are planned. The engine never reads global state, environment variables, or terminal properties. Cancellation is cooperative.
- **Documents.** Analysis, plan, report, and manifest are versioned JSON documents (`kind` plus `schemaVersion`). They form the stable contract for tools and future frontends.
- **Progress events.** The engine emits structured progress events (phase started, file parsed, operation emitted). The CLI renders them to the terminal. A planned NDJSON event stream exposes them to other programs.
- **CLI.** Parses arguments, calls the engine, renders results, and maps outcomes to exit codes. It contains no migration logic.
- **Future frontends.** A GUI or editor integration can link the engine as a library, or call the CLI with JSON output and the event stream. Either way it uses the same engine, so there is no second migration implementation (`D-005`). No frontend is in scope today (`D-004`).

## 19. Extensibility

- **Source adapters** provide discovery signatures, framework fact extraction, the feature catalog entries, and source-side assessment.
- **Target adapters** provide target profiles, path conventions, printers for target-specific file types, configuration generation, and target-specific verification steps.
- **Rule packs** belong to a migration path and use the model's neutral concepts where possible and facets where necessary.
- **Knowledge base updates** extend coverage without code changes when a new package or API fits existing strategies.
- **Plugins** are deferred (`DF-002`). Options to evaluate in V2: compile-time only (status quo), an out-of-process protocol, or WebAssembly modules. A plugin mechanism will be introduced only if third-party rule development becomes a real need, because it adds security and compatibility surface.
- **Optional model assistance** connects at exactly one seam: after deterministic planning, for occurrences already classified `unknown` or `review`. A model may only choose among deterministic strategies that are already implemented, or route an occurrence to human review. It never edits output directly (see [LLM_POLICY.md](LLM_POLICY.md)).

## 20. Concurrency, determinism, and performance

- **Parallelism.** Per-file stages (reading, parsing, per-file fact extraction) and per-operation emission run in parallel. Global steps (graph construction, path mapping, assessment propagation) run after these per-file stages finish.
- **Deterministic merge.** File IDs are assigned from the sorted list of normalized paths before parallel work starts. Parallel results are stored by ID, never by completion order. The implemented `KeyTable` assigns such dense identities and `DenseSlots` accepts one result per identity in any order (`D-060`); discovery returns its admitted files as such a table (`D-064`).
- **No order dependence.** No output may depend on hash map iteration order, filesystem enumeration order, scheduling, locale, wall-clock time, or random values. Collections that are serialized or iterated for output are sorted by a total order. A workspace lint disallows the standard hash collections; the parser's lookup tables use wrappers without iteration (`D-060`).
- **Limits and budgets.** Resource limits are sizes and counts, never times. Cumulative budgets are charged by one owner in canonical order, so the input that exceeds a limit does not depend on scheduling (`D-061`). The single-threaded discovery walker owns its five budgets and charges them in byte-wise path order (`D-064`).
- **Integer metrics.** Percentages are computed from integer numerators and denominators with a specified truncation rule. No floating point is involved, so results are identical across platforms (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)).
- **Memory.** Compact node representations, string interning for identifiers and paths, and per-file allocation patterns that free memory in bulk. Performance budgets are in [ENGINEERING.md](ENGINEERING.md).

## 21. Architecture review: key questions

1. **Is a universal IR justified for the MVP?** No. V1 uses a project model sized for one migration path, with neutral naming and framework facets. The IR is extracted in V2 once a second source or target exists (section 10).
2. **How do rules stay deterministic across different execution models?** Rules are pure functions of model facts, options, and versioned data. Each one encodes an explicit, documented semantic mapping with preconditions. Where the engine cannot statically prove a precondition, the rule classifies the occurrence as `review` or `unsupported`, and does so deterministically. Determinism guarantees reproducibility, not correctness. Correctness comes from preconditions, postconditions, and verification.
3. **What must be preserved for auditability?** Snapshot hash, file content hashes, byte spans of consumed source, the rule ID and version for each operation, the provenance of every output region, versions of the engine, ruleset, knowledge base, and target profile, the options that affect output, and the reason for every classification (sections 12, 13, and 14).
4. **How is coverage measured without misleading percentages?** Assessment, execution, and verification are separate status dimensions. Every percentage has an explicit numerator and denominator. Unknown is always shown. Fixed, documented weights are used, and bulk verbatim files are excluded from headline figures. Values are truncated, never rounded up (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)).
5. **How do dependency incompatibilities influence the plan?** Through usage, not declaration. Code that uses an incompatible package is classified accordingly, and route readiness reflects this transitively. Retained packages keep their resolved versions. Incompatible packages are reported with alternatives where the knowledge base knows any (section 11).
6. **How are silent regressions prevented?** Fail-closed classification, rule postconditions, residue scans, route parity checks, golden and regression tests in Frameshift's own CI, verification levels with explicit scope, and reports that never claim more than the checks that ran (sections 11, 12, and 16).
7. **What is the smallest useful capability?** An accurate analysis-only report for a Next.js App Router project, which tells users how migratable their project is and why. After that comes the migration of static routes, layouts, links, global styles, and public assets, verified structurally and by build (see [ROADMAP.md](ROADMAP.md)).
8. **How do we avoid third-party libraries without unmaintainable code?** Build only bounded, well-specified components: parsers for documented formats, a specified semver grammar, gitignore matching. Write the specification first, test against public conformance suites, fuzz, and document limitations. Never build a type checker, bundler, or network package resolver (see [ENGINEERING.md](ENGINEERING.md)).
9. **Which language?** Rust is the primary language for all stages (`D-018`). Other languages may be used for specific, important tasks behind clear boundaries when they offer a demonstrable quality advantage (`D-019`). Full reasoning is in [ENGINEERING.md](ENGINEERING.md).
10. **How are new framework adapters added without rewriting the core?** The layering of section 6 places framework knowledge only in adapters, rule packs, and knowledge base data. The core works on neutral concepts and opaque facets. V2 proves this by adding a second source and a second target without core changes that are specific to a migration path.
11. **Where would optional model assistance connect?** At a single post-planning seam, limited to choosing among implemented deterministic strategies or routing to review (section 19, [LLM_POLICY.md](LLM_POLICY.md)).
12. **How does Frameshift stay CLI-first but reusable?** The engine API and versioned JSON documents are the contract. The CLI is one adapter with no migration logic (section 18).
13. **What is undecidable or unverifiable?** See section 22.
14. **How is an optimized target distinguished from a behavior-preserving conversion?** Every operation is either a *conversion* (intended to preserve behavior) or an *optimization* (intended to improve the result while changing observable characteristics, such as hydration timing or image formats). Conversions are the default. Optimizations are opt-in, reported in a separate category, and never counted as plain migration. Example: `client:load` preserves Next.js hydration timing, while `client:visible` is an optimization (`P-009`).

## 22. Limits of automated migration

These limits are fundamental, not temporary gaps:

- **Behavioral equivalence is undecidable in general.** By Rice's theorem, no algorithm can decide whether two arbitrary programs behave identically. Frameshift can only apply mappings that are correct under stated preconditions, and collect evidence through tests and comparisons.
- **Dynamic code defeats static analysis.** Computed import specifiers, `eval`, dynamically built class names (for example `` `bg-${color}-500` ``), reflection over props, and data-dependent rendering cannot be fully analyzed statically. They are reported as `unknown`.
- **Runtime environment differences.** Request-time data, cookies, and headers have no equivalent in static output. Build-time data fetched from external services can change between builds of source and output, so comparisons can show differences that have nothing to do with the migration.
- **Third-party behavior.** The internal behavior of dependencies is outside the analyzed code. Compatibility claims about packages are evidence-based approximations.
- **Visual and interaction equivalence.** CSS cascade order, layout, animation timing, focus management, and hydration timing can differ even when markup matches. Static output comparison does not cover them. Runtime verification can cover some of them, never all.
- **Intent.** Code does not record why it was written. Whether a `'use client'` directive is necessary, or a loading state matters, often depends on intent only humans know. Such cases are `review` items.

## 23. Open architectural questions

Open questions are tracked in [DECISIONS.md](DECISIONS.md), notably weight values (`Q-002`) and static output comparison thresholds and HTML parsing (`Q-005`). Parser recovery policy is accepted in `D-034`; CSS synchronization and budgets are implemented under `D-078`; JavaScript synchronization and budgets remain M3 implementation work.
