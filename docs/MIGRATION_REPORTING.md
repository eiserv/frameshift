# Migration Reporting

**Status: reporting specification with catalog identifiers, unit identities, and finding records implemented.** `FindingCode`, `RuleId`, and `FeatureId` exist under `D-055`, `UnitKind` and `UnitId` under `D-056`, and bounded findings and deterministic report-local numbering under `D-057` (sections 15 to 17 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). Assessments, metrics, the evidence ledger, and migration report documents remain planned. This document defines how Frameshift counts, classifies, measures, and reports migrations. Reporting is a core product feature: the numbers must be useful, reproducible, and impossible to misread as a guarantee of correctness. Related decisions: `D-009`, `P-006`, and `P-016` in [DECISIONS.md](DECISIONS.md).

## 1. Principles

1. **A percentage is never a statement of behavioral equivalence.** A migration can be 100% complete and verified at every available level and still differ from the source in ways no automated check covers. Every human-readable report says so.
2. **Every percentage has an explicit numerator and denominator.** Both are present in JSON output, and their definitions are fixed in this document.
3. **Unknown is always visible.** Every distribution includes an `unknown` share, even when it is zero.
4. **Separate dimensions stay separate.** Pre-execution feasibility, execution outcome, verification, and review are reported as distinct dimensions. They are never blended into a single "score".
5. **Conservative display.** Displayed values are truncated, never rounded up. A value of 99.96% is shown as 99.9%, and 100.0% appears only when every counted unit qualifies.
6. **Reproducible.** The same input and versions produce the same numbers on every platform. Integer arithmetic only.
7. **No probabilistic confidence.** Frameshift does not emit confidence scores. A classification is a deterministic result of documented rules, and the reason is always stated.

## 2. Counting basis

### 2.1 Units

A unit is the smallest independently reported element of a migration. Units do not overlap: every analyzed piece of source belongs to exactly one unit, so shares add up to the whole.

| Unit kind | Covers | Unit ID example |
| --- | --- | --- |
| `route` | One URL pattern and its page module | `route:/blog/[slug]` |
| `layout` | One layout module | `layout:app/blog/layout.tsx` |
| `provider` | A component whose main role is providing shared state (context provider) | `provider:components/cart/CartProvider.tsx#CartProvider` |
| `endpoint` | One route handler | `endpoint:/feed.xml` |
| `component` | One component definition outside route and layout modules, with its render context | `component:components/Hero.tsx#Hero` |
| `route-file` | A special route file other than pages and layouts (`not-found`, `loading`, `error`, `template`, generated metadata files) | `route-file:app/loading.tsx` |
| `module` | Non-component code: utilities, data access, constants | `module:lib/posts.ts` |
| `stylesheet` | One CSS, SCSS, or CSS Module file | `stylesheet:app/globals.css` |
| `config` | One configuration concern, such as image handling, trailing slashes, TypeScript options, or scripts | `config:nextjs.images` |

Unit ID values are implemented (`D-056`, section 16 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). They derive from kind, exact canonical source path and optional export name, original rooted URL pattern for routes/endpoints, or a namespaced configuration concern, never from counters. Within the subject and export, ASCII letters, digits, and `-._~/[]()` remain literal; every other original UTF-8 byte is encoded as uppercase `%HH`. This distinguishes literal `#`, `%`, `:`, controls, and Unicode bytes without normalizing source names. For example, file `a#b` becomes `component:a%23b`, export `b` of file `a` becomes `component:a#b`, and file `a%23b` becomes `component:a%2523b`. Constructors validate identity structure and serialized byte limits, not actual discovery or framework route semantics. The counting model itself remains planned.

### 2.2 Inventory items

Some elements are counted and listed but excluded from the weighted headline metrics, because including them would distort the numbers:

| Inventory item | Why excluded from headline metrics | Where reported |
| --- | --- | --- |
| Dependency | Its effect on migration shows up in the units that use it. Counting it as a unit as well would count the same risk twice. | Dependency section, with compatibility classes |
| Verbatim file (`public/` assets, copied data files) | Copying is trivial and nearly certain. Hundreds of images would inflate feasibility without reflecting migration difficulty. | File accounting |
| Out-of-scope file (tests, CI workflows, linter configuration in V1) | Explicitly outside the stage's scope, and reported as such rather than mixed into feasibility | Out-of-scope section with counts and manual tasks |
| Ignored path (`.git/`, `node_modules/`, build outputs, environment files, `.gitignore` matches) | Not part of the project source, or never opened for security reasons | Ignored list with reasons: `vcs`, `dependencies`, `build-output`, `environment`, or `gitignore` (implemented in discovery, `D-064`) |

### 2.3 File accounting invariant

Every file under the source root falls into exactly one of four groups: belongs to one or more units, verbatim, out of scope, or ignored. The report shows this accounting. Its totals must match discovery exactly. A file that fits nowhere is an engine defect and is reported as `FS9001`.

## 3. Status model

### 3.1 Assessment (before execution)

Every feature occurrence, and therefore every unit, has exactly one assessment status:

| Status | Meaning |
| --- | --- |
| `auto` | Automatically migratable. A rule handles it deterministically, and its preconditions are verified statically. |
| `review` | Migratable with review. A rule transforms it, but correctness depends on facts that cannot be verified statically, or the mapping changes a behavior that may matter. |
| `unsupported` | Recognized, and known to be impossible to migrate in the current stage. |
| `unknown` | Not recognized, not analyzable, or not determinable. |

A unit's status is the most severe status among its occurrences, in the order `auto` < `review` < `unknown` < `unsupported`. `unsupported` ranks above `unknown` because it is a definite blocker. All occurrences remain listed regardless.

### 3.2 Execution outcome

Every unit has exactly one execution outcome after `migrate`:

| Outcome | Meaning |
| --- | --- |
| `not-run` | Analysis or planning only, or a dry-run |
| `migrated` | Every operation for the unit succeeded and every feature was transformed |
| `partial` | Output emitted, but at least one feature could not be transformed. A manual task exists. |
| `preserved` | Not migrated (`unsupported` or `unknown`). The source is copied to `.frameshift/unmigrated/`. |
| `failed` | An operation or postcondition failed. The failure is recorded. |
| `skipped` | Excluded by an explicit user option, or not committed because `--strict` aborted the run |

### 3.3 Verification

Verification status is recorded per unit and per level (`structural`, `build`, `static-output`, and `runtime`, all in V1 scope) as `passed`, `failed`, `not-run`, or `not-applicable`. Levels are defined in section 16 of [ARCHITECTURE.md](ARCHITECTURE.md). A unit is verified at a level only if every check at that level covering the unit ran and passed:

- `structural`: per-unit checks such as re-parsing, import resolution, residue scans, and route parity.
- `build`: the project build covers every emitted unit.
- `static-output`: a unit is covered by every route that renders it, and it is verified only if all of those routes were compared and matched. A unit rendered by no route (such as a configuration concern) is `not-applicable`.

### 3.4 Review

A unit assessed as `review` that was migrated is *awaiting review*. In V1 it stays in that state in every report. Recording human acknowledgments is planned for V2 (`DF-008`).

### 3.5 User-facing categories

The categories users see map onto these dimensions as follows. Execution categories are mutually exclusive. Verification and review are separate views across them.

| Category shown to users | Definition |
| --- | --- |
| Automatically migratable | Assessment `auto` |
| Migratable with review | Assessment `review` |
| Unsupported | Assessment `unsupported` |
| Unknown | Assessment `unknown` |
| Migrated | Outcome `migrated` |
| Partially migrated | Outcome `partial` |
| Not migrated | Outcome `preserved` or `skipped` |
| Failed | Outcome `failed` |
| Migrated and verified (level L) | Outcome `migrated`, and verification at level L `passed` |
| Migrated, not yet verified | Outcome `migrated`, and no level beyond `structural` has passed |
| Awaiting review | Outcome `migrated`, and assessment `review` |

## 4. Metrics

Let *W(S)* be the summed weight of units in set *S*, and *T* the total weight of all units. All metrics below are weighted unless marked "count".

| Metric | Formula | Answers |
| --- | --- | --- |
| Assessment distribution | *W(auto)*, *W(review)*, *W(unsupported)*, *W(unknown)*, each divided by *T* | How migratable is the project before anything is written? The four shares add up to the whole. |
| Analysis coverage | *W(auto ∪ review ∪ unsupported) / T* | How much of the project did Frameshift understand? |
| Automatic feasibility | *W(auto) / T* | How much can migrate with no human judgment? |
| Feasibility with review | *W(auto ∪ review) / T* | How much can Frameshift transform at all? |
| Rule coverage (count) | distinct detected feature kinds that have a transformation rule / distinct detected feature kinds | How well does the current ruleset cover the constructs this project uses? Unknown constructs count as distinct kinds. |
| Route readiness (count) | Routes per readiness class, out of all routes | How many pages can move, including everything they depend on? |
| Execution distribution | *W(migrated)*, *W(partial)*, *W(preserved ∪ skipped)*, *W(failed)*, each divided by *T* | How much was actually migrated? The shares add up to the whole. |
| Migration completion | *W(migrated) / T* | The headline execution figure. Partial units count as zero. |
| Plan completion | *W(migrated) / W(auto ∪ review)* | How much of what was attempted succeeded? Secondary, always shown next to migration completion. |
| Awaiting review | *W(migrated ∩ review) / T* | How much migrated output still needs human judgment? |
| Verified at level L | *W(migrated ∩ passed at L) / T* | How much migrated output passed the checks at level L? Units not applicable at L are reported next to this metric. |
| Dependency compatibility (count) | Direct dependencies per compatibility class | Which packages help or hinder the migration? |

Denominators are always the total weight *T*, except for plan completion, which says so in its name. A metric whose denominator is zero is shown as "n/a", never as 0% or 100%.

### 4.1 Route readiness

For each route, the readiness class looks at the route unit and its transitive closure: layout chain, page module, imported modules and components, providers, and stylesheets.

| Class | Condition |
| --- | --- |
| Ready, automatic | Every unit in the closure is `auto` |
| Ready, with review | Every unit is `auto` or `review`, and at least one is `review` |
| Blocked | At least one unit is `unsupported` |
| Uncertain | No unit is `unsupported`, and at least one is `unknown` |

A route can be locally `auto` and still blocked through a shared dependency. Reports show the blocking unit and the dependency path, for example `/shop` blocked by `provider:components/cart/CartProvider.tsx#CartProvider` through `components/ProductGrid.tsx`.

## 5. Weighting

### 5.1 Alternatives considered

| Basis | Assessment |
| --- | --- |
| Unweighted unit counts | Simple and transparent. However, a decorative icon component counts as much as a route or shared state. Shown as a secondary view, never alone. |
| Lines of code | Misleading. Long static pages dominate, and short but critical state logic barely registers. Formatting style changes the result. Rejected. |
| Complexity metrics (cyclomatic complexity, JSX node counts) | Measures size, not migration significance. Hard to explain and unstable across refactorings. Rejected for V1. |
| Usage-based criticality (fan-in) | Plausible, but adding one route changes the weights of unrelated components, which makes results hard to compare between runs. Rejected for V1. |
| Fixed weights per unit kind | Explainable, reproducible, comparable across projects and runs. Coarse, because it ignores size within a kind. Chosen (`P-006`). |

### 5.2 Weights v1 (proposed)

Weights express the significance and risk of a unit for the behavior of the migrated site. They do not estimate effort.

| Unit kind | Weight | Rationale |
| --- | --- | --- |
| `route` | 8 | A user-visible page. Failures are directly visible. |
| `provider` | 8 | Shared state affects many components, and failures are subtle. |
| `layout` | 5 | Affects every route below it, but structurally simple |
| `component` (client context) | 5 | Interactive behavior, and hydration risks |
| `endpoint` | 4 | Generates files that other systems consume (feeds, JSON) |
| `component` (server context) | 3 | Static rendering, lower behavioral risk |
| `route-file` | 2 | Secondary route behavior (loading, error, not-found) |
| `module` | 2 | Supporting code. Its importance shows up through the units that use it. |
| `stylesheet` | 2 | Visual output. Mostly copied, with ordering risks. |
| `config` | 2 | Configuration concern |

Rules for weights:

- Weights are integers, published in this document and in every report (`weights-v1`).
- Weights are not tuned per project. Users can view unweighted counts, but cannot change weights, so reports stay comparable.
- Changing any weight creates a new weights version, recorded in the decision register.
- The values above are proposals to be validated against real projects in M5 (`Q-002`).

## 6. Calculation and display

- **Integer arithmetic.** Each metric is computed as `basisPoints = floor(numerator × 10000 / denominator)`. JSON includes the numerator, denominator, and basis points.
- **Display.** One decimal place, truncated: `floor(basisPoints / 10) / 10`. For example 6165 basis points display as 61.6%.
- **Small values.** A share greater than zero but below 0.1% is shown as `<0.1%`, never `0.0%`.
- **Sums.** Truncated shares may sum to slightly less than 100.0% (for example 99.8%). Reports never adjust values to force a sum of 100%. Exact numerators are always shown next to percentages.
- **Bars.** Distribution bars have a fixed width. Characters are allocated to the classes by the largest remainder method, and every bar is accompanied by its numerators.

## 7. Project-level verdicts

A verdict summarizes the project in one or two sentences. It is derived from facts, not percentage thresholds:

- **Good candidate:** no blocked routes, or blocked routes affect only isolated features.
- **Good candidate with manual work:** blocked routes exist, and the report names them and the features that block them.
- **Poor candidate for a static migration:** most routes depend on request-time features. The verdict names those features and the share of routes they affect.

Every verdict line cites the findings it rests on.

## 8. Findings

### 8.1 Structure

| Field | Content |
| --- | --- |
| `id` | Report-local identifier (`F0001`, `F0002`, ...), assigned only after the complete ordering described below; at least four digits, expanding above 9999 |
| `code` | Stable finding code, such as `FS3120` |
| `severity` | `error`, `blocker`, `unknown`, `review`, or `info` |
| `unit` | Affected unit ID, if established; absent for project or file conditions without a unit |
| `feature` | Feature ID, where applicable |
| `location` | Project-wide, file-only (root-relative), or validated text range. Text offsets are zero-based bytes and canonical, with an exclusive end. Derived lines and columns are one-based, counting Unicode scalar values (`D-029`). Missing positions are never invented. |
| `message` | One-line summary |
| `explanation` | Why this matters, in plain language |
| `remediation` | Concrete options for the user |
| `rule` | The rule that produced or was blocked by the finding, if any |

The foundation record and collector exist (`D-057`, IMPLEMENTATION.md section 17). Ordering compares severity (`error`, `blocker`, `unknown`, `review`, `info`), location, code, unit, feature, rule, message, explanation, and remediation. Locations compare project scope first, then original UTF-8 path bytes, file-only before text ranges on the same path, start/end byte offsets, then derived start/end line and column. Optional associations sort absent first. Strings compare original bytes, without normalization. Identical records remain separate and receive successive IDs. These IDs belong to one finalized list and are not persistent keys. The full report schema and renderers remain planned.

The record requires nonblank prose, with a single-line control-free message and no NUL in any field. Explanation and remediation may contain line breaks and other controls, which future renderers must escape. Inclusive defaults are 64 KiB per prose field and 100,000 records per collector, counting duplicates. Size/count/allocation failures return explicit errors without truncating evidence.

The following assessment and outcome propagation remains planned. Severities correspond to statuses: `blocker` makes the unit `unsupported`, `unknown` makes it `unknown`, and `review` makes it `review`. `info` does not change the status. `error` marks a Frameshift operational failure (the unit becomes `failed`), or an internal defect.

### 8.2 Finding code ranges

| Range | Area | Examples |
| --- | --- | --- |
| `FS1xxx` | Input and discovery | `FS1003` symbolic link escapes the project root. `FS1004` file exceeds the size limit. `FS1010` lockfile format not parsed. |
| `FS2xxx` | Parsing | `FS2001` syntax error, file not analyzed. `FS2002` unsupported text encoding. |
| `FS3xxx` | Analysis and compatibility | `FS3001` unrecognized construct. `FS3002` dynamic import with a computed specifier. `FS3003` configuration value cannot be evaluated statically. `FS3101` `cookies()` requires request-time rendering. `FS3102` `headers()` requires request-time rendering. `FS3110` dynamic route parameters cannot be enumerated at build time. `FS3120` Server Action cannot run in static output. `FS3130` proxy or middleware affects this route. `FS3201` context shared across islands. `FS3210` navigation changes to full page loads. `FS3301` dependency incompatible with Preact. `FS3302` dependency compatibility unknown. `FS3401` image placeholder not migrated. `FS3402` font fallback metrics may differ. |
| `FS4xxx` | Planning | `FS4001` ambiguous or overlapping rules (fail closed). `FS4002` output path collision. |
| `FS5xxx` | Transformation and emission | `FS5001` generated output failed to re-parse. `FS5002` rule postcondition failed. |
| `FS6xxx` | Verification | `FS6101` residual source-framework API in output. `FS6102` import does not resolve in output. `FS6103` route missing in output. `FS6201` build failed. `FS6301` static output differs for a route. |
| `FS7xxx` | Output and filesystem safety | `FS7001` output location exists. `FS7002` output location inside the source. `FS7003` unsafe output path rejected. `FS7004` managed output was modified since generation. |
| `FS9xxx` | Internal | `FS9001` internal error. Results of this run are not trustworthy. |

The discovery walker, the snapshot store, the package loader, workspace detection, and the Next.js source adapter emit the following assigned codes (`D-064`, `D-065`, `D-067`, `D-069`, `D-073`, `D-074`, sections 22, 24, 26, 30, and 31 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). Locations are the named path, a text range in it where the problem has a span, or, for entries directly in the root, project scope. Package and workspace findings name fields, kinds, paths, releases, and limits, never package names, patterns, or values. Severities follow the status mapping above: `unknown` where content is not analyzed, `error` for operational failures, and `info` where nothing is lost.

| Code | Severity | Meaning |
| --- | --- | --- |
| `FS1001` | unknown | An entry name or path cannot be represented as a canonical source path; reported at its parent directory without the name. A directory is not traversed. |
| `FS1002` | info | Device file, FIFO, or socket skipped. |
| `FS1003` | unknown | Link target leaves the project root. |
| `FS1004` | unknown | File exceeds the per-file size limit; accounted for, not read. |
| `FS1005` | info | Link not followed; its target inside the project is recorded. |
| `FS1006` | unknown | Link target not resolved: absolute, through another link or an untraversed directory, unreadable, unrepresentable, or discovery incomplete. |
| `FS1007` | info | Link target does not exist. |
| `FS1008` | unknown | Windows reparse point that is not a link (for example a cloud placeholder) not followed. |
| `FS1009` | unknown | Directory exceeds the depth limit and was not traversed. |
| `FS1010` | unknown | Lockfile format or format version not parsed: `bun.lockb`, npm version 1, Bun version 0, pnpm versions other than 9.0, a Yarn classic version above 1, a Yarn Berry version that no stable release writes, a `yarn.lock` with neither the classic header nor `__metadata`, or an unknown version; versions fall back to declared ranges (`D-069`, `D-072`). |
| `FS1011` | unknown | File count limit reached; discovery stopped. |
| `FS1012` | unknown | Total size limit reached; discovery stopped. |
| `FS1013` | unknown | Entry limit reached; discovery stopped. |
| `FS1014` | unknown | `.gitignore` byte limit reached; discovery stopped. |
| `FS1015` | unknown | `.gitignore` matching work limit reached; discovery stopped. |
| `FS1016` | error | Directory could not be read; its contents were not discovered. |
| `FS1017` | error | Entry could not be inspected. |
| `FS1018` | error | `.gitignore` file could not be read; its directory was not traversed. |
| `FS1019` | error | File could not be read by the snapshot store; its content is not analyzed (`D-067`). |
| `FS1020` | unknown | A `package.json` field or entry has an unexpected type or value, or the document is not an object; the value is not used (`D-069`). |
| `FS1021` | review | Duplicate key in a package file. JSON uses the last occurrence, as npm does; YAML and Yarn classic lockfiles are rejected as ambiguous (`D-071`, `D-072`). |
| `FS1022` | unknown | `packageManager` is not `<name>@<version>[+<hash>]` with npm, pnpm, yarn, or bun and an exact version; it is ignored. |
| `FS1023` | review | Lockfile of another package manager than the `packageManager` field names; it is ignored. |
| `FS1024` | unknown | Lockfiles of several package managers and no valid `packageManager` field; no lockfile is used. Reported at the directory. |
| `FS1025` | info | Lockfile superseded by one of the same manager: `npm-shrinkwrap.json` over `package-lock.json`, `bun.lock` over `bun.lockb`. |
| `FS1026` | unknown | Lockfile structure differs from its format; the lockfile is not used. |
| `FS1027` | review | Lockfile root declarations differ from `package.json`; resolved versions may be stale. For Yarn classic, a declaration has no pattern in the lockfile, which also holds for one that a workspace package satisfies (`D-072`). |
| `FS1028` | unknown | Lockfile exceeds `package.max-lockfile-bytes`; it is not parsed. |
| `FS1029` | review | The `yarn.lock` format belongs to another Yarn generation than `packageManager` names: Yarn 1 ignores a Berry lockfile, and Yarn 2 or later migrates a classic one, so the next install may resolve other versions (`D-072`). |
| `FS1030` | unknown | A workspace pattern uses syntax outside the evaluated grammar, or a form that the release reads unlike its documented semantics; no member of the declaration is determined (`D-073`). Located at the pattern. |
| `FS1031` | unknown | A workspace pattern could reach an entry that discovery did not traverse (ignored, a link, or not read), or discovery is incomplete; members may be missing. |
| `FS1032` | unknown | The package manager release is not determined and the candidate releases disagree on this directory; its membership is not known. |
| `FS1033` | review | The workspace patterns select other directories when names compare without case, as on Windows and macOS. |
| `FS1034` | review | A workspace declaration of another package manager than the detected one is ignored. |
| `FS1035` | review | The package manager would reject the workspace: a non-private Yarn 1 root, duplicate member names, a member manifest that is not valid, a Bun member without a name or a listed directory without `package.json`, or an invalid `pnpm-workspace.yaml`. |
| `FS1036` | unknown | A matched directory's manifest is hidden from discovery or only in a format that Frameshift does not read (`yarn.json`, `package.yaml`, `package.json5`), or a member's nested declaration is unusable. |
| `FS1037` | review | `packageManager` names a release whose workspace semantics are not modeled; every modeled release of that manager is evaluated. |
| `FS1038` | unknown | A workspace limit (`workspace.max-patterns` or `workspace.max-match-work`) is reached; members are not determined. |
| `FS1040` | review | Next.js evidence is incomplete: `next` is declared without a configuration file or route files, or a configuration file exists without the declaration; the package is not classified as Next.js (`D-074`). |
| `FS1041` | review | The Next.js version lies outside the validated range 14 to 16 (`D-023`). |
| `FS1042` | review | The Next.js version is imprecise or unknown: no lockfile resolution, so the lower bound of the declared range is used, or a source other than the registry. |
| `FS1043` | review or info | The Next.js configuration file is not loaded as expected: a name that the version ignores or rejects, or `next.config.mts`, which depends on Node.js; `info` when no configuration exists inside the root. |
| `FS1044` | review | A Next.js router directory is not fully known: not a traversed directory, untraversed entries below it, or `app` and `pages` in different directories, which Next.js 15 and later reject. |
| `FS2001` | unknown | Syntax error; the file is not analyzed. The package loader reports it for JSON, JSONC, YAML, and Yarn classic package files, including their parse limits and Yarn classic files with merge conflict markers. |
| `FS2002` | unknown | Unsupported text encoding (not UTF-8); the file is not analyzed. |
| `FS2003` | unknown | File exceeds the text size limit; it is not decoded or analyzed. |

`FS2001` to `FS2003` keep their meaning for every parsed file kind as later parsers adopt them. Finding code syntax is implemented independently of this future catalog: exactly `FS` followed by four ASCII digits (`D-055`). A valid value alone does not establish that a code is assigned. Codes are never reused for a different meaning. `frameshift explain <code>` prints the full documentation for a code (see [CLI.md](CLI.md)).

## 9. Report contents

A full report contains the following sections. Each one is a projection of the same evidence ledger.

1. **Summary:** tool and ruleset versions, source and target identity, verdict, assessment distribution, route readiness, dependency summary, rule coverage, and file accounting. After execution and verification it also shows the execution distribution and verification results.
2. **Routes:** per route, its readiness, units in its closure, findings, output path, and verification results.
3. **Units:** per unit, its kind, weight, sources, assessment, strategy, rules, outcome, verification, and findings.
4. **Files:** per source file, its units or inventory class, output paths, and findings.
5. **Dependencies:** per direct dependency, the declared range, resolved version, compatibility class with evidence, usage sites, and the planned change (kept, removed, replaced, or added).
6. **Configuration changes:** each configuration concern with source values (where statically known) and the generated target configuration.
7. **Rules applied:** each rule with its version and the units it handled.
8. **Findings:** all findings, filterable by severity, code, unit, and file.
9. **Verification:** per level, the checks run, their results, tool versions, and logs (truncated, stored separately).
10. **Manual tasks:** a deduplicated, ordered list with references to findings.
11. **Generated helpers:** files in `src/frameshift-compat/` with their purpose and removal guidance.

## 10. Formats

- **Human-readable CLI output:** summary-first, compact, ASCII by default. Colors only on terminals that support them, honoring `NO_COLOR`. See [CLI.md](CLI.md).
- **Markdown:** `.frameshift/SUMMARY.md` in the output project, plus `frameshift report --format markdown` for sharing.
- **JSON:** versioned documents with `kind` and an integer `schemaVersion`. The CLI shell implements `frameshift.version` and usage `frameshift.error` v1 (`D-050`, [CLI.md](CLI.md), section 4.9), with schemas in [frameshift.version.schema.json](../schemas/frameshift.version.schema.json) and [frameshift.error.schema.json](../schemas/frameshift.error.schema.json). The migration documents `frameshift.analysis`, `frameshift.plan`, `frameshift.report`, and `frameshift.manifest` remain planned. Adding fields does not change the schema version; consumers accept additional fields. Removing or changing the meaning of a field increments it and counts as a breaking change (see [RELEASES.md](RELEASES.md)). A formal JSON Schema for each remaining document kind is planned alongside its first implementation.

### 10.1 Determinism and privacy

- Everything except the `run` section (timestamps, durations) is byte-identical across repeated runs with the same inputs. Determinism tests exclude only `run`.
- Paths are relative to the source root. Absolute paths, user names, and host names never appear in reports, plans, or manifests.
- JSON reports contain no source code snippets by default. `--include-snippets` adds short snippets around findings. Terminal output shows snippets locally (see [SECURITY.md](SECURITY.md)).

## 11. Sample reports

> **Fictional example data.** The projects `lumen-site` and `harbor-portal`, their files, packages (including `motion-lite`), numbers, hashes, and version strings are invented to illustrate formats and metric definitions. Frameshift 0.9.0 is a placeholder version. None of these commands exist yet.

### 11.1 Analysis summary

```text
$ frameshift analyze ./lumen-site --to astro-preact

Frameshift 0.9.0  analyze  nextjs 16.2.1 (App Router) -> astro-preact (profile astro7-preact10)

Project       ./lumen-site
Package mgr   pnpm (pnpm-lock.yaml, lockfile 9.0)
Files         201 accounted: 118 in units, 71 verbatim, 12 out of scope
Ignored       .git/ (vcs), node_modules/ (dependencies), .next/ (build output), out/ (build output)

Verdict
  Good candidate with manual work: 4 of 24 routes are blocked.
  Blocking features: cookies() on 2 routes [FS3101], a Server Action on 1 route [FS3120],
  a context provider shared across islands affecting 1 route [FS3201].

Assessment (86 units, total weight 412, weights-v1)
  [#####################################===============xxxxx???]
   #  auto          61.6%   254/412   56 units
   =  review        24.2%   100/412   20 units
   x  unsupported    9.2%    38/412    6 units
   ?  unknown        4.8%    20/412    4 units
  Analysis coverage 95.1%   Feasibility with review 85.9%

Routes (24, including dependencies)
   ready, automatic     11
   ready, with review    8
   blocked               4   /account  /account/settings  /contact  /shop
   uncertain             1   /lab

Dependencies (40 direct, including 3 framework packages that the migration replaces)
   compatible 22   via preact/compat 9   incompatible 2   unknown 4

Rule coverage   36 of 41 detected feature kinds (87.8%)

Blockers and unknowns (10 units; 5 shown, use --all)
   blocker  FS3101  cookies() requires request-time rendering     app/account/page.tsx:8:17
   blocker  FS3120  Server Action cannot run in static output     app/contact/page.tsx:5:19
   blocker  FS3201  Context shared across 3 islands               components/cart/CartProvider.tsx:12:1
   unknown  FS3002  Dynamic import with a computed specifier      app/lab/page.tsx:14:22
   unknown  FS3302  Compatibility of motion-lite 2.4.1 unknown    components/Hero.tsx:3:1

A high feasibility value does not mean the migrated site will behave identically.
Run `frameshift explain <code>` for details, or add `--report report.json` for the full report.
```

For comparison, the unweighted shares of the same project (`--unweighted`) are: auto 65.1% (56/86), review 23.2% (20/86), unsupported 6.9% (6/86), unknown 4.6% (4/86).

### 11.2 Route and file views

```text
$ frameshift report report.json --section routes

Route                 Readiness            Closure  Findings
/                     ready, automatic          9   -
/about                ready, automatic          5   -
/account              blocked                   6   blocker FS3101
/account/settings     blocked                   7   blocker FS3101
/blog                 ready, with review        7   review FS3402
/blog/[slug]          ready, with review        8   review FS3402, review generateStaticParams composition
/contact              blocked                   4   blocker FS3120
/lab                  uncertain                 3   unknown FS3002
/shop                 blocked                   6   blocker FS3201 via components/ProductGrid.tsx
...
```

```text
$ frameshift report report.json --file components/Hero.tsx

components/Hero.tsx
  unit       component:components/Hero.tsx#Hero   client context, weight 5
  status     unknown
  strategy   Preact island (client:load), not planned while unknown
  findings
    unknown  FS3302  Compatibility of motion-lite 2.4.1 with Preact is unknown   3:1
             No knowledge base entry exists for this package. Frameshift cannot confirm that it
             works through preact/compat. Check it manually, or replace it.
  rendered by  route:/ (app/page.tsx), route:/about (app/about/page.tsx)
```

### 11.3 Dependencies

```text
$ frameshift report report.json --section dependencies

Package               Declared   Resolved   Class          Used in   Plan
clsx                  ^2.1.1     2.1.1      compatible     14 files  keep
next                  16.2.1     16.2.1     framework      41 files  remove
react                 19.2.0     19.2.0     framework      38 files  alias to @preact/compat
react-dom             19.2.0     19.2.0     framework       2 files  alias to @preact/compat
react-hook-form       ^7.62.0    7.62.0     compat          3 files  keep
motion-lite           ^2.4.0     2.4.1      unknown         2 files  keep, see FS3302
next-auth             ^5.1.0     5.1.0      incompatible    2 files  keep, see FS3301 (both in blocked routes)
...
```

### 11.4 Migration and verification

```text
$ frameshift migrate ./lumen-site --to astro-preact --out ../lumen-site-astro

Frameshift 0.9.0  migrate  nextjs 16.2.1 (App Router) -> astro-preact (profile astro7-preact10)
Plan 3f9c2a71   143 operations   input snapshot 8b41e0d2

Execution (86 units, total weight 412)
   migrated        83.4%   344/412   74 units
   partial          2.4%    10/412    2 units
   not migrated    14.0%    58/412   10 units   sources preserved in .frameshift/unmigrated/
   failed           0.0%     0/412    0 units
  Plan completion 97.1% (344/354)

Awaiting review   21.8%   90/412   18 units
Verification      structural passed for 344/412 (83.4%); build and static-output not run
Manual tasks      7, listed in ../lumen-site-astro/.frameshift/SUMMARY.md

Output committed to ../lumen-site-astro
```

```text
$ frameshift verify ../lumen-site-astro --level static-output --source ./lumen-site --allow-exec

Frameshift 0.9.0  verify  ../lumen-site-astro
Notice: this installs dependencies and runs production builds of both projects. Their packages and
        configuration files execute code on this machine. Builds run on temporary copies.

structural      passed   344/412 (83.4%)
build           passed   pnpm install --ignore-scripts, astro check, astro build
static-output   18 of 20 compared routes match
                /blog/[slug]   FS6301  1 link target differs: /blog/intro (source) vs /blog/intro/ (output)
                /pricing       FS6301  text differs in 1 element: whitespace around an inline link

Verified at static-output   65.0%   268/412
Not applicable               1.4%     6/412   configuration units
Result: verification found differences (exit code 1). Details in .frameshift/report.json.
```

### 11.5 JSON excerpt

```json
{
  "kind": "frameshift.report",
  "schemaVersion": 1,
  "tool": {
    "name": "frameshift",
    "version": "0.9.0",
    "ruleset": "nextjs-to-astro-preact@0.9.0",
    "knowledgeBase": "2026.10.1",
    "targetProfile": "astro7-preact10",
    "weights": "weights-v1"
  },
  "run": {
    "command": "analyze",
    "startedAt": "2026-10-11T19:02:44Z",
    "durationMs": 734
  },
  "input": {
    "snapshot": "sha256:8b41e0d2c7a9...",
    "files": { "accounted": 201, "inUnits": 118, "verbatim": 71, "outOfScope": 12 },
    "ignored": [
      { "path": ".git/", "reason": "vcs" },
      { "path": ".next/", "reason": "build-output" },
      { "path": "node_modules/", "reason": "dependencies" },
      { "path": "out/", "reason": "build-output" }
    ]
  },
  "source": {
    "adapter": "nextjs",
    "version": "16.2.1",
    "versionSource": "lockfile",
    "router": "app",
    "packageManager": "pnpm"
  },
  "target": { "id": "astro-preact", "profile": "astro7-preact10" },
  "summary": {
    "totals": { "units": 86, "weight": 412 },
    "assessment": {
      "auto": { "units": 56, "weight": 254, "basisPoints": 6165, "unitBasisPoints": 6511 },
      "review": { "units": 20, "weight": 100, "basisPoints": 2427, "unitBasisPoints": 2325 },
      "unsupported": { "units": 6, "weight": 38, "basisPoints": 922, "unitBasisPoints": 697 },
      "unknown": { "units": 4, "weight": 20, "basisPoints": 485, "unitBasisPoints": 465 }
    },
    "metrics": {
      "analysisCoverage": { "numerator": 392, "denominator": 412, "basisPoints": 9514 },
      "automaticFeasibility": { "numerator": 254, "denominator": 412, "basisPoints": 6165 },
      "feasibilityWithReview": { "numerator": 354, "denominator": 412, "basisPoints": 8592 },
      "ruleCoverage": { "numerator": 36, "denominator": 41, "basisPoints": 8780 }
    },
    "routes": { "total": 24, "readyAuto": 11, "readyReview": 8, "blocked": 4, "uncertain": 1 },
    "dependencies": { "direct": 40, "framework": 3, "compatible": 22, "compat": 9, "incompatible": 2, "unknown": 4 },
    "verdict": {
      "class": "good-candidate-with-manual-work",
      "findings": ["F0001", "F0002", "F0003", "F0004"]
    }
  },
  "units": [
    {
      "id": "route:/contact",
      "kind": "route",
      "weight": 8,
      "sources": [{ "file": "app/contact/page.tsx", "sha256": "4d1f09ab..." }],
      "assessment": "unsupported",
      "strategy": "preserve",
      "rules": [],
      "readiness": "blocked",
      "findings": ["F0003"]
    }
  ],
  "findings": [
    {
      "id": "F0003",
      "code": "FS3120",
      "severity": "blocker",
      "unit": "route:/contact",
      "feature": "nextjs.api.server-action",
      "location": {
        "file": "app/contact/page.tsx",
        "start": { "offset": 111, "line": 5, "column": 19 },
        "end": { "offset": 122, "line": 5, "column": 30 }
      },
      "message": "Server Action cannot run in static output",
      "explanation": "Server Actions execute on a server for each request. A static Astro site has no server.",
      "remediation": [
        "Post the form to an external form service.",
        "Post the form to a serverless function on your hosting platform."
      ],
      "rule": null
    }
  ]
}
```

### 11.6 A poor candidate

```text
$ frameshift analyze ./harbor-portal --to astro-preact

Frameshift 0.9.0  analyze  nextjs 15.5.4 (App Router) -> astro-preact (profile astro7-preact10)

Verdict
  Poor candidate for a static migration: 9 of 12 routes depend on request-time features.
  cookies() in app/(app)/layout.tsx affects 7 routes [FS3101]; proxy.ts matches /dashboard/:path*
  (8 routes) [FS3130]; Server Actions are used on 5 routes [FS3120].
  Consider a later Frameshift stage with server-capable targets, or migrate the 3 public routes only.

Assessment (58 units, total weight 234, weights-v1)
   auto            21.3%    50/234
   review          10.6%    25/234
   unsupported     61.5%   144/234
   unknown          6.4%    15/234

Routes (12, including dependencies)
   ready, automatic      2
   ready, with review    1
   blocked               9
   uncertain             0
```

### 11.7 `.frameshift/SUMMARY.md` excerpt

```markdown
# Migration summary

Migrated from Next.js 16.2.1 (App Router) to Astro + Preact (profile astro7-preact10) by Frameshift 0.9.0.
Plan 3f9c2a71. Full report: `.frameshift/report.json`.

A successful build does not prove that this site behaves exactly like the original.
Review the items below before deploying.

## Manual tasks

1. Run `pnpm install` to update the copied lockfile.
2. Port the Server Action in `app/contact/actions.ts` (preserved in `.frameshift/unmigrated/app/contact/`). See FS3120.
3. Replace request-time cookie handling on `/account` and `/account/settings`. See FS3101.
4. Replace the cart context shared across islands in `components/cart/CartProvider.tsx`. See FS3201.
5. Check whether `motion-lite` works with Preact, or replace it. See FS3302.
6. Update your hosting configuration: build command `astro build`, output directory `dist/`.
7. Rename public environment variables in your hosting settings as listed in the report.

## Review checklist (18 items)

- [ ] `src/pages/blog/[slug].astro`: the parameter composition for nested `generateStaticParams` matches the source.
- [ ] `components/Card.tsx`: the `icon` slot is only rendered, not inspected or cloned.
- [ ] Site-wide: islands no longer keep state across navigation (FS3210).
- ...
```

## 12. Interpreting the numbers

- **Automatic feasibility** says how much the current ruleset can handle without human judgment. It says nothing about how hard the remainder is.
- **Migration completion** says how much output was produced without known gaps. It does not mean the output is correct.
- **Verified at level L** says which checks passed. Static output comparison does not cover interactivity, styling, or timing.
- **Rule coverage** describes Frameshift's coverage of this project's constructs, not the project's quality.
- A project at 100% on every metric still needs human acceptance testing before deployment.

## 13. Open questions

- Final weight values (`Q-002`).
- Static output comparison normalization and thresholds (`Q-005`).
