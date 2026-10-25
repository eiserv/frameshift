# Command-Line Interface

**Status: read-only inspection, analysis, and reporting implemented (`D-100`).** Available commands are `help`, `version`, `inspect`, `analyze`, `report`, `explain`, and assessment `rules`. Planning, migration, and verification commands and their options remain specified below and are rejected as usage errors. Analysis has no execution side effects; explicit report files are separate CLI artifacts. Exact contracts and limits are in IMPLEMENTATION.md sections 32 and 51.

The CLI is Frameshift's only user interface in the current scope (`D-004`). It is meant to become a polished, production-quality developer tool, not a temporary prototype.

## 1. Design principles

- **Thin adapter.** The CLI parses arguments, calls the engine API, renders results, and maps outcomes to exit codes. It contains no migration logic, so a future frontend can reuse the engine without a second implementation (`D-005`, section 18 of [ARCHITECTURE.md](ARCHITECTURE.md)).
- **Safe by default.** Commands that read are the default. Commands that write need an explicit output location. Running external tools needs an explicit authorization flag. The source project is never modified.
- **Non-interactive.** V1 never prompts. Every decision is made through flags, so the same command behaves identically in a terminal and in CI.
- **Separated streams.** Results go to standard output. Progress, notices, and errors go to standard error. With `--format json`, standard output contains exactly one JSON document.
- **Stable machine output.** JSON documents are versioned (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)). Human-readable text may change between releases. Scripts should rely on JSON.
- **Deterministic.** Apart from progress output and the `run` metadata section, the same command on the same input produces the same output.

## 2. Command overview

| Command | Purpose | Writes files | Runs external tools |
| --- | --- | --- | --- |
| `inspect` (implemented) | Identify a project: framework, version, router, package manager, structure | No | No |
| `analyze` | Assess migratability to a target: units, statuses, metrics, findings | Only with `--report` | No |
| `plan` | Compute the migration plan: operations, output paths, manual tasks | Only with `--out` or `--report` | No |
| `migrate` | Execute the plan into a new output directory | Yes, output directory only | No |
| `verify` | Verify a migrated project at a chosen level | Only reports and temporary copies | Only with `--allow-exec` |
| `report` | Render a stored report document in other formats or views | Only with `--out` | No |
| `rules` | List and document migration rules and supported features | No | No |
| `explain` | Explain a finding code, rule, or feature | No | No |
| `version` (implemented) | Print version information | No | No |
| `help` (implemented) | Print help | No | No |

Commands that were considered and not included in V1:

- `init`: the configuration file is small and described by a JSON Schema, so a generator adds little.
- `diff`: covered by `migrate --dry-run --show-files` and by the plan document.
- `targets`: covered by `rules` and `inspect`, which list available migration paths.
- `review`: recording review acknowledgments is planned for V2 (`DF-008`).
- `doctor`: environment checks for verification can be added if user feedback shows a need.

## 3. Global options

`--help`/`-h`, `--version`, `--format`, and the command options listed as implemented below exist. Analysis/report options accept separate values or `=`, before or after the command; native source and report paths need not be UTF-8. Duplicate or incompatible options are usage errors. Analysis and stored report output support `text` and `json`; stored reports also support `markdown`. `--report` on `analyze` and `--out` on `report` create a new file and never overwrite. An analysis report destination must be outside the source root; a stored-report destination may not replace any existing file. Color, snippets, verbosity, and progress controls in the table remain planned; current output is uncolored and has no progress stream.

| Option | Default | Meaning |
| --- | --- | --- |
| `--format text\|json` | `text` | Output format on standard output |
| `--report <file>` | none | Also write the full JSON document for this command to a file |
| `--color auto\|always\|never` | `auto` | Color in text output. `auto` enables color only on terminals. `NO_COLOR` is honored. |
| `--quiet`, `-q` | off | Print only errors and the final result |
| `--verbose`, `-v` | off | Print additional detail |
| `--no-progress` | off | Disable progress output. Progress is also disabled automatically when standard error is not a terminal. |
| `--include-snippets` | off | Include short source snippets around findings in JSON output |
| `--help`, `-h` | | Help for the command |
| `--version` | | Short version string |

## 4. Commands

### 4.1 `inspect`

**Implemented (`D-075`).**

```text
frameshift inspect [<source>] [--package <dir>] [--format text|json]
```

Fast, read-only identification of a project. `<source>` defaults to the current directory, may be any native path, and is the workspace root: nothing above it is read (`D-073`). No target is needed. Inspection reads only what identification requires: directory listings, `.gitignore` files, `package.json` files, the selected lockfile, and `pnpm-workspace.yaml`. It never writes, runs project code, or uses the network, and it computes no snapshot identity.

| Option | Meaning |
| --- | --- |
| `--package <dir>` | Inspect one package: a workspace member given relative to `<source>` with `/` separators, or the root with `.`. The value is never normalized; a backslash, a trailing slash, or `..` is a usage error. Without the option, the root and every member are inspected. |
| `--format text\|json` | Text (default) or the `frameshift.inspection` document |

The text output reports the files that discovery accounted for, the package manager with its source, the lockfile with its format version and the lockfiles that are ignored, the workspace with its members and the evaluated package manager releases, and for each package its Next.js verdict, version with its evidence, configuration file, and routers with their route file counts, followed by every finding on one line. It never prints the source path, and source-derived strings are escaped; the JSON document spells DEL, C1 controls, line and paragraph separators, and bidirectional formatting characters as \u escapes (`D-076`). The following output is the golden of the fictional fixture `inspect-next-workspace`, with its engine version filled in:

```text
$ frameshift inspect ./harbor

Frameshift 0.1.0  inspect

Files          10 admitted (1619 bytes), 9 directories, 2 ignored, 0 other
Package mgr    npm 11.11.0 (packageManager field)
Lockfile       package-lock.json (format 3)
Workspace      3 members, evaluated as npm-11

Package        . (fictional-harbor)
  Framework    no supported framework detected

Package        apps/docs (fictional-docs)
  Framework    Next.js evidence incomplete
  Config       none
  Router       no route files

Package        apps/site (fictional-site)
  Framework    Next.js 15.5.2 (locked)
  Config       apps/site/next.config.ts
  Router       App Router in apps/site/app (1 route file); Pages Router in apps/site/pages (1 route file)

Package        packages/ui (fictional-ui)
  Framework    no supported framework detected

Migration      no migration path ships in this build
Findings       1 (1 review)
  review  FS1040  apps/docs/package.json  Next.js evidence incomplete
```

`--format json` prints one `frameshift.inspection` document, schema version 1, described by [frameshift.inspection.schema.json](../schemas/frameshift.inspection.schema.json): the engine version, discovery counts, the package manager and lockfile, the workspace, one entry per inspected package with its Next.js detection, an empty `migrationPaths` array until a ruleset ships, and every finding with its ID, code, severity, location, and prose. Paths are relative to `<source>`, and `.` names the root.

A project without a supported framework, an incomplete inventory, and unusable package files are valid inspection results: the command reports what it found and exits with 0. A source that is missing, unreadable, or not a directory, a file that changes during the run, a `--package` directory that is not a determined member, and a finding count beyond the collector limit exit with 3; an engine defect exits with 70. With `--format json`, these failures also print a `frameshift.error` document with the exit code.

Language, styling, static export, and file accounting by unit, which an earlier sample of this section showed, need configuration evaluation (M4) and unit classification (M5) and are not reported yet.

### 4.2 `analyze`

```text
frameshift analyze [<source>] --to <target> [--from <source-adapter>]
                   [--package <dir>] [--format text|json] [--report <new-file>]
                   [--fail-on <conditions>] [--unweighted] [--all]
```

| Option | Meaning |
| --- | --- |
| `--to <target>` | Required. Target identifier, for example `astro-preact`. Required even while only one target exists, so commands stay unambiguous later. |
| `--from <adapter>` | Override source detection, for example `nextjs`. Normally detected. |
| `--fail-on <conditions>` | Comma-separated: `unknown`, `unsupported`, `review`, `blocked-routes`. The command exits with 1 if any listed condition occurs in a unit or finding, including findings without a unit. Finding severities `blocker`, `unknown`, and `review` match `unsupported`, `unknown`, and `review`; `blocked-routes` considers route readiness only. |
| `--unweighted` | Show unweighted unit counts as the primary view |
| `--all` | List all findings instead of the top entries |

**Implemented (`D-100`).** The source is the workspace root; `--package <dir>` selects one proven member, and `--package .` selects the root. Without it, the root must itself contain an App Router directory. Unknown source versions and Pages Router evidence remain visible. `--from nextjs` is the only source override, and `--to astro-preact` is required. The output is `frameshift.analysis` v1 ([schema](../schemas/frameshift.analysis.schema.json)); there is no migration execution or verification evidence. `--report` writes that document only to a new destination outside the source.

Output: verdict, assessment distribution, route readiness, dependencies, rule coverage, and top findings. The larger illustrative sample is in section 11.1 of [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md). Analysis finding unsupported features is a successful result, so the exit code is 0 unless `--fail-on` says otherwise.

### 4.3 `plan`

```text
frameshift plan [<source>] --to <target> [--out <plan.json>] [--show operations|files|tasks]
```

Computes the plan without writing project files. `--out` writes the plan document for review or approval. A plan document is bound to the source snapshot, engine version, ruleset, and options (see section 13 of [ARCHITECTURE.md](ARCHITECTURE.md)).

```text
$ frameshift plan ./lumen-site --to astro-preact --out plan.json

Frameshift 0.9.0  plan  nextjs 16.2.1 (App Router) -> astro-preact (profile astro7-preact10)
Plan 3f9c2a71   143 operations   86 units   input snapshot 8b41e0d2

create     src/layouts/RootLayout.astro          layout.chain                 from app/layout.tsx
create     src/pages/index.astro                 route.static-page            from app/page.tsx
create     src/pages/blog/[slug].astro           route.dynamic-static-params  from app/blog/[slug]/page.tsx   review
transform  components/Counter.tsx                component.preact-island      from components/Counter.tsx
copy       public/ (71 files)                    asset.public-verbatim
config     astro.config.mjs                      config.astro
config     package.json                          config.package-manifest
preserve   .frameshift/unmigrated/app/contact/   preserve.unmigrated          4 files              FS3120
... 135 more (use --show operations)

Manual tasks: 7 (use --show tasks)
Plan written to plan.json
```

### 4.4 `migrate`

```text
frameshift migrate [<source>] --to <target> --out <dir>
                   [--plan <plan.json>] [--dry-run] [--show-files]
                   [--strict] [--on-existing fail|replace-managed] [--clean-staging]
```

| Option | Meaning |
| --- | --- |
| `--out <dir>` | Required. The output directory. It must not exist or must be empty, must not be inside the source, must not contain the source, and must not be a filesystem root or the home directory. |
| `--plan <file>` | Execute only if the recomputed plan matches this plan document. Otherwise exit with 3. The file is an approval artifact, not an instruction stream. |
| `--dry-run` | Run everything except the commit. Nothing is written unless `--report` is given. |
| `--show-files` | List every file operation |
| `--strict` | Abort before commit if any unit is `unsupported`, `unknown`, or `failed`. Nothing is written, and the exit code is 1. |
| `--env-names astro\|keep` | Naming of public environment variables (`D-021`). Default `astro` renames `NEXT_PUBLIC_X` to `PUBLIC_X`. `keep` preserves the original names. Also accepted by `plan`. |
| `--config <file>`, `--no-config` | Use a specific configuration file, or none (section 5.1). Also accepted by `analyze` and `plan`. |
| `--on-existing fail\|replace-managed` | `fail` (default) refuses an existing output. `replace-managed` replaces an existing output only if its manifest marks it as Frameshift output and no file in it was modified since generation. The previous output is moved aside, never deleted. |
| `--clean-staging` | Remove a staging directory left by an interrupted run. Only directories carrying Frameshift's staging marker are removed. |

Structural verification always runs as part of `migrate`, because its checks are rule postconditions. A sample is in section 11.4 of [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).

```text
$ frameshift migrate ./lumen-site --to astro-preact --out ../lumen-site-astro --dry-run --show-files

Dry run: nothing will be written.
create     src/layouts/RootLayout.astro                  layout.chain             from app/layout.tsx
create     src/pages/index.astro                         route.static-page        from app/page.tsx
transform  components/Counter.tsx                        component.preact-island  from components/Counter.tsx
copy       public/favicon.ico                            asset.public-verbatim    from public/favicon.ico
config     astro.config.mjs                              config.astro
preserve   .frameshift/unmigrated/app/contact/page.tsx   preserve.unmigrated      from app/contact/page.tsx
...
143 operations planned; 0 files written.
```

### 4.5 `verify`

```text
frameshift verify <output-dir> [--level structural|build|static-output|runtime] [--source <dir>]
                  [--allow-exec] [--allow-install-scripts] [--timeout <duration>] [--keep-temp]
```

| Option | Meaning |
| --- | --- |
| `--level` | Highest level to run. Levels are cumulative. Default `structural`. |
| `--source <dir>` | Required for `static-output` and `runtime`. The source snapshot must match the hash recorded in the output manifest; otherwise the command exits with 3. |
| `--allow-exec` | Required for `build`, `static-output`, and `runtime`. Authorizes running the package manager, framework build tools, and, for runtime checks, a user-installed browser on temporary copies. These tools execute third-party code from the projects' dependencies and configuration files. |
| `--allow-install-scripts` | Allow package lifecycle scripts during installation. By default, installation disables them where the package manager supports it. |
| `--timeout <duration>` | Limit for each external command, for example `15m`. A timeout fails the corresponding check. |
| `--keep-temp` | Keep temporary copies for debugging. They are removed by default. |

Without `--allow-exec`, requesting `build`, `static-output`, or `runtime` is a usage error (exit code 2). Installation needs network access to the package registry. Frameshift prints a notice before any external tool runs. Runtime is required V1 scope in M10, covering hydration, declared interactions, and selected accessibility checks. Browser configuration and the check protocol remain open until M10. The full security model is in [SECURITY.md](SECURITY.md).

### 4.6 `report`

```text
frameshift report <report.json | output-dir> [--format text|markdown|json] [--out <file>]
                  [--section summary|routes|units|files|dependencies|config|rules|findings|verification|tasks]
                  [--route <pattern>] [--file <path>] [--severity <minimum>] [--code <code>]
                  [--unweighted] [--all]
```

**Implemented for `frameshift.analysis` v1 (`D-100`).** Renders stored evidence, with bounded schema, accounting, metric, and cross-reference validation. It never re-analyzes or opens source paths from the document. Text summaries show at most ten findings unless `--all` is set; section views print escaped JSON records as text, Markdown uses a protected code block, and JSON emits the complete document by default or the selected/filtered records. `--route`, `--file`, `--severity`, and `--code` filter the available records; without a section, a filter selects findings. Execution reports from future milestones are not accepted yet. Given an output directory, it reads `.frameshift/report.json`. Samples are in section 11.2 of [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).

### 4.7 `rules`

```text
frameshift rules [--to <target>] [--unsupported]
frameshift rules show <rule-id>
```

**Implemented for assessment metadata (`D-100`).** Lists the feature predicates with their version, feature, and allowed catalog statuses. These predicates assess source; they emit no operations and do not count as transformation rule coverage. Transformation-rule listing and the detailed transformation examples below remain planned. `--unsupported` lists features that are detected but not migratable in the current stage. `show` prints a rule's documentation, preconditions, examples, and verification requirements.

```text
$ frameshift rules --to astro-preact

nextjs-to-astro-preact   ruleset 0.9.0   target profile astro7-preact10   42 rules

Rule                          Version  Handles                    Possible status
route.static-page             3        nextjs.route.page          auto
route.dynamic-static-params   2        nextjs.route.dynamic       auto, review
route.catch-all               1        nextjs.route.catch-all     auto, review
layout.chain                  2        nextjs.file.layout         auto
metadata.static               4        nextjs.metadata.static     auto
link.anchor                   2        nextjs.api.link            auto, review
...
Detected but unsupported in this stage: 17 features (frameshift rules --unsupported)
```

### 4.8 `explain`

**Implemented (`D-100`).** Finding codes, concrete feature IDs, and full assessment rule IDs have offline explanations. Unknown identifiers are usage errors and are never echoed into output.

```text
frameshift explain <finding-code | rule-id | feature-id>
```

```text
$ frameshift explain FS3201

FS3201  Context shared across islands                              severity: blocker

What it means
  A React context provider supplies state to components that become separate Astro islands.
  Each island is an independent Preact root, so context does not cross island boundaries.

Why it is not migrated automatically in this stage
  Preserving the behavior requires replacing the context with shared module state. That changes
  component code in ways that need design decisions.

What you can do
  - Move the provider and all its consumers into one client component, so they form one island.
  - Replace the context with a small shared store module that each island imports.

Planned
  Automatic migration to a shared store is planned for V2.
```

### 4.9 `version` and `help`

**Implemented.** `frameshift version` prints the engine version, shipped rulesets, knowledge bases and target profiles, build commit, and the actual Rust compiler used to build the engine. The current catalogs are empty: the E2 driver does not ship product migration data. The build commit is unknown in ordinary local builds; CI and packaging explicitly embed the checked-out commit (`D-051`). The version is the workspace version, 0.1.0 since the first release. Release Please manages versions; none is bumped manually.

`frameshift version --format json` produces one LF-terminated UTF-8 `frameshift.version` document with `schemaVersion: 1`, in the field order below. The schema is [frameshift.version.schema.json](../schemas/frameshift.version.schema.json). Consumers must accept additional fields.

```json
{
  "kind": "frameshift.version",
  "schemaVersion": 1,
  "engineVersion": "0.1.0",
  "rulesets": [],
  "knowledgeBases": [],
  "targetProfiles": [],
  "buildCommit": null,
  "toolchain": "rustc <actual version, commit and date>"
}
```

The `toolchain` value above is illustrative. It is captured at build time, never discovered from the runtime host. `frameshift --version` prints `frameshift 0.1.0` for this release and rejects every additional argument, including `--format`.

`frameshift`, `frameshift help`, `frameshift --help`, and `frameshift -h` print the command overview. `frameshift help help` prints help usage. `frameshift help version`, `frameshift version --help`, and `frameshift version -h` print version usage, and `frameshift help inspect`, `frameshift inspect --help`, and `frameshift inspect -h` print inspect usage. Help is text only. Unavailable help topics, unavailable commands, unexpected operands, malformed native Unicode arguments, unknown options, and duplicated options exit with 2. Successful output goes only to standard output; errors go to standard error. Diagnostics never echo user argument text.

If JSON was explicitly selected anywhere in an invalid invocation, standard output contains one `frameshift.error` v1 document with `kind`, `schemaVersion`, `exitCode: 2`, and a static English `message`. Standard error still contains the human-readable error. The schema is [frameshift.error.schema.json](../schemas/frameshift.error.schema.json). In text mode, usage errors leave standard output empty. A standard-stream write or flush failure exits with 6 and cannot guarantee a complete JSON document.

## 5. Input and output handling

- **Paths.** Arguments accept native paths on every platform. Reports and documents use forward-slash paths relative to the source root, never absolute paths.
- **Text output.** UTF-8, ASCII symbols by default. Control characters in file names and source-derived strings are escaped before printing, so source content cannot inject terminal escape sequences.
- **JSON output.** With `--format json`, a failing command (exit codes 2 to 6, and 70) writes a `frameshift.error` document to standard output and a human-readable message to standard error.
- **Progress events.** The engine emits structured progress events. A machine-readable NDJSON event stream (`--events <file>`) is planned for integrations and future frontends, and will be specified before implementation.
- **Environment variables.** The CLI reads only presentation-related variables (`NO_COLOR`, and terminal detection). Environment variables never change analysis or migration results.
- **Configuration file.** Optional `frameshift.jsonc`, see section 5.1.
- **Interruption.** On Ctrl+C, the CLI stops cooperatively, removes its staging directory, and leaves the output location untouched. A hard kill can leave a marked staging directory, which `--clean-staging` removes.

### 5.1 Configuration file

V1 supports an optional project configuration file (`D-027`):

- **Name and location.** `frameshift.jsonc` in the source root, or any path given with `--config <file>`. `--no-config` ignores the file.
- **Format.** JSON with comments, validated against a published JSON Schema. A `$schema` property enables editor completion. Unknown keys are errors, not warnings.
- **Contents.** Options that describe the project and the desired migration, for example the target, excluded paths, and `envNames` (`"astro"` or `"keep"`, `D-021`). The exact key set is specified in M6.
- **Precedence.** Command-line flags override the file, and the file overrides defaults. The effective options are recorded in the plan and the report, so results stay reproducible.
- **Safety.** The file is part of the source project, which may be untrusted. It can never enable `--allow-exec` or `--allow-install-scripts`, change overwrite policies, choose the output location, or reference paths outside the source root. Those decisions stay with the person running the command.

## 6. Safety options at a glance

| Concern | Default behavior | Opt-in |
| --- | --- | --- |
| Source modification | Never | None. There is no option to write into the source. |
| Existing output | Refused | `--on-existing replace-managed`, which protects modified output |
| Partial results | Committed with failures reported | `--strict` aborts instead |
| External processes | Never | `--allow-exec` for `verify` |
| Package lifecycle scripts | Disabled during verification installs | `--allow-install-scripts` |
| Network access | None by Frameshift itself | Only external tools under `--allow-exec` |
| Source snippets in JSON | Excluded | `--include-snippets` |

## 7. Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Completed, but a requested condition failed: `--fail-on` matched, `--strict` aborted the commit, or verification failed or found differences |
| 2 | Usage error: invalid arguments, a missing required option, or a missing authorization flag for a requested action |
| 3 | Input error: source missing or unreadable, a source that changed during the run, or an unknown `--package` (implemented for `inspect`); no supported framework for `analyze`, `plan`, or `migrate`, an invalid or stale plan document, or a source that no longer matches a plan or manifest (planned) |
| 4 | Output refused: output location exists or is unsafe, a staging directory from an interrupted run is present, or managed output was modified |
| 5 | Completed with failures: output committed, but one or more units failed |
| 6 | Environment error: a standard-stream write or flush failed (implemented), or a required external tool is missing or failed before its checks could run (planned) |
| 70 | Internal error: an engine defect (finding `FS9001`). Results of this run are not trustworthy. 70 follows the BSD `sysexits` convention for internal software errors. `inspect` exits with 70 when the engine reports a defect (implemented). |
| 130 | Interrupted by the user. Nothing was committed. |

When several conditions apply, the code earliest in this order wins: 70, 130, 2, 3, 4, 6, 5, 1, 0.

## 8. CI usage

Illustrative commands (not functional yet):

```bash
frameshift analyze . --to astro-preact --format json --report frameshift-analysis.json --fail-on unsupported,unknown
```

```bash
frameshift migrate . --to astro-preact --out ../site-astro --strict --report frameshift-report.json
```

```bash
frameshift verify ../site-astro --level build --allow-exec --timeout 20m
```

Recommendations for CI:

- Use `--format json` and parse documents, not text output.
- Keep JSON reports as private CI artifacts. They contain file paths and findings about the project (see [SECURITY.md](SECURITY.md)).
- Run `verify --allow-exec` only in disposable environments such as containers or ephemeral CI runners.

## 9. Options intentionally absent from V1

- Hydration optimization (for example choosing `client:visible`), because optimizations are opt-in and planned later.
- Model-assisted processing, which is deferred and would always be opt-in (see [LLM_POLICY.md](LLM_POLICY.md)).
- Interactive prompts or a terminal UI, because V1 is non-interactive by design.
