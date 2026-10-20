# Testing and Verification Strategy

**Status: strategy with implemented M0 and M1 contract tests and the first M2 discovery, package, workspace, and Next.js detection tests and product fixtures.** The workspace tests immutable text and positions, the lexical scanners and all parser dialects, character references and generated tables, span rewriting, JSX conversion and Astro printing, corpus contracts, the development tools, CLI help/version behavior, canonical source/portable output paths, stable identifiers, findings, bounded SHA-256 hashing, deterministic collections, run limits with the reconciled limit set, `.gitignore` matching against Git's verdicts, read-only discovery on in-memory trees, real directories created at test time, and the product fixtures below `fixtures/discovery/`, the read-once snapshot store and its identity, JSON and JSONC parsing, and the package model with npm ranges, `package.json`, package manager detection, and npm, Bun, pnpm 9.0, and Yarn classic and Berry lockfiles, together with full YAML 1.2 syntax and the Yarn classic lockfile syntax (`D-071`, `D-072`), workspace members with the pattern semantics of eight package manager releases, and Next.js framework, version, configuration, and router detection (`D-073`, `D-074`). The tests cover deterministic results, exact byte spans and provenance, fail-closed errors, resource limits, native argument encoding, and process-level CLI goldens. Native debug/release suites, E2 hash comparison, and packaging run on all six CI platforms (`D-051`). Current commands and validation evidence are in [IMPLEMENTATION.md](IMPLEMENTATION.md).

The coverage-guided fuzzer has eighteen targets, including `paths` for arbitrary-byte path validation against an independent acceptance model (`D-054`), `identifiers` for catalog grammar and percent-encoded unit identity against independent models (`D-055`, `D-056`), `hashes` for SHA-256 streaming, limits, failed-update preservation and independent canonical digest grammar (`D-059`), `collections` for key tables, dense slots, and budgets against independent models (`D-060`, `D-061`), `gitignore` for `.gitignore` decisions and work charges against an independent recursive model (`D-065`), and `json` for both JSON dialects against an independent recursive-descent model (`D-068`), `semver` for npm ranges, versions, and specifiers against an independent precedence model (`D-070`), `package` for the manifest and lockfile models (`D-069`), `yaml` for syntax streams, graph references, limits, the JSON subset, and the pnpm and Yarn Berry models (`D-071`, `D-072`), `yarn` for the Yarn classic syntax and model with a write and reparse round trip (`D-072`), and `workspace` for the workspace semantics of every release over directory listings (`D-073`). Instrumented runs have historical Windows evidence, short Linux x86_64 `identifiers` and `paths` smoke runs (IMPLEMENTATION.md section 16), and Windows smoke runs for `hashes`, `collections`, `gitignore`, `json`, `semver`, `package`, `yaml`, `yarn`, and `workspace` (sections 19, 21, 23, and 25 to 30); fuzzing in CI remains planned. Real-world files are kept outside the repository and are not committed test fixtures. Feature and composite fixtures, executor safety, framework builds, scheduled fuzzing, and behavioral comparisons below are planned unless explicitly marked as implemented. Product verification is defined in section 16 of [ARCHITECTURE.md](ARCHITECTURE.md).

## 1. Goals

**M1 CI (`D-051`, `D-052`).** The implemented CI workflow runs the existing debug and release test suites on all six documented native platforms, validates generated corpus contracts and E2 goldens, and compares E2 output hashes. It also validates both Go helpers and smoke-tests help and version from packaged binaries. Current live evidence is in section 13 of [IMPLEMENTATION.md](IMPLEMENTATION.md). The remaining product fixture, framework build, behavioral comparison, fuzz schedule, and release hardening categories below are still planned.

1. **Correctness:** rules produce the specified output for their inputs.
2. **Honesty:** no unit is ever classified `auto` when it is not safely migratable. A false `auto` is the most serious class of defect.
3. **Determinism:** identical inputs produce byte-identical results on every platform.
4. **Safety:** the source is never modified, nothing is written outside the output location, and no code runs without authorization.
5. **Robustness:** hostile or malformed input never crashes, hangs, or exhausts the engine.
6. **Performance:** the budgets in [ENGINEERING.md](ENGINEERING.md) are met and regressions are visible.
7. **Portability:** identical behavior on Linux, Windows, and macOS, on x86_64 and ARM64.

## 2. Principles

- Test behavior and contracts, not implementation details. Every test has a maintenance cost, so tests should be targeted and proportionate.
- Each test should fail for one clear reason.
- Prefer fixtures that look like real projects over synthetic fragments, but keep each fixture as small as its purpose allows.
- Every bug fix comes with a regression test that fails without the fix.
- Tests are deterministic. A flaky test is a defect and is fixed or removed, never retried until it passes.

## 3. Test categories

### 3.1 Unit tests

Six SHA-256 public-facade tests (`D-059`) check standard known answers, binary/BOM/line-ending preservation, chunk boundaries and cloned prefixes, zero/exact/cumulative limits, canonical parsing with exhaustive ASCII substitutions, raw-byte round trips, and ordering. A core test covers the algorithm length ceiling without allocating exabytes. The native CI also runs the facade tests with the portable software backend forced. The `hashes` fuzz target checks facade invariants and digest grammar, not an independent implementation of cryptography.

Six collection facade tests (`D-060`) check identity assignment for every input permutation, byte-wise order, bound and duplicate precedence, insertion and worker completion order independence, rejected and missing results, and input-free errors. Seven limit facade tests (`D-061`) check documented defaults, sorted and complete canonical entries, the inclusive boundary and fixed order of every limit relation, the attained unit identity bound, and budget atomicity, including overflow (IMPLEMENTATION.md sections 20 and 21).

The eleven finding facade tests (`D-057`) cover validated Unicode/BOM/CRLF/EOF locations, invalid spans, optional typed associations, exact prose limits and error precedence, complete comparison of every record field, preservation of evidence on failed insertion, input-order independence with duplicates, and report-local numbering above 9999 (IMPLEMENTATION.md section 17).

The eleven implemented public-facade path contract tests (`D-054`) exercise canonical source paths, separate portable output validation, exact bytes and offsets, resource boundaries, and deterministic collection keys without filesystem access. Their scope is recorded in section 14 of [IMPLEMENTATION.md](IMPLEMENTATION.md). Native path conversion, actual long-path I/O, collision detection, and containment tests remain future filesystem work.

Pure functions and small components: path normalization, semver matching, gitignore matching, the static evaluator, metric arithmetic, the edit engine, and individual rule preconditions. Fast, exhaustive where the input space is small.

### 3.2 Parser fixtures and conformance

- **Fixture files** per syntax area (modules, expressions, JSX, TypeScript types, error cases), with expected tree snapshots and span checks. For every node, slicing the original text by the node's span must yield exactly that construct.
- **Conformance suites:** public test suites with compatible licenses, such as the TypeScript compiler's conformance tests (Apache-2.0) and test262, serve as parser inputs. Expected outcome: files the reference implementation accepts must parse, and files it rejects must not produce a tree that the engine would transform. These suites are fetched or vendored with license notices, and are never shipped.
- **Round-trip property:** applying an empty edit set reproduces the input byte for byte.
- The same applies to the CSS, JSONC, YAML 1.2, and lockfile parsers, each against its specification.
- **Future recovery tests (`D-034`):** multiple errors in source order, bounded diagnostic and recovery work, forward progress, refusal to synchronize in an unknown lexical context, and exclusion of the entire erroneous file from transformations even when a partial tree is recovered.

The measurement harness's `--check` mode checks the complete generated JavaScript, JSX, TypeScript, TSX, and declaration-file corpora without timing. It verifies source-spanning programs, unique node reachability, nested and source-ordered spans, node spans that neither start nor end with trivia, template segments, comment delimiters, identical repeated parses, and byte-exact empty edits with full provenance. The unit suite applies the same checks to 720 files across 40 seeds and covers empty sources, Unicode, CRLF, tagged invalid escapes, a failed parse, conflicting tool modes, and all-empty memory reports. See section 6 of [IMPLEMENTATION.md](IMPLEMENTATION.md) for the command and limits. Development-time oracle comparisons outside the repository complement these checks: acceptance with V8, `@babel/parser`, and the pinned TypeScript compiler, and node spans with `@babel/parser` for JSX and with the pinned TypeScript compiler for the TypeScript dialects (section 5 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).

### 3.3 Fuzzing

Every parser of untrusted input and the static evaluator are fuzzed. The fuzzer is a purpose-built, coverage-guided development tool, `tools/frameshift-fuzz` (`D-044`, section 7 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). It exists for the JavaScript, JSX, TypeScript, and TSX parser, the regular-expression and lexical scanners, the character reference decoders, the span edit core, and the JSX conversion with Astro printing. Each target checks the documented contracts of its entry points, and a supervising process saves crashes, hangs, and contract violations with a replay command. The JSON and JSONC parser, npm ranges, and the package manifest and npm and Bun lockfile models are fuzzed (`json`, `semver`, `package`); the `yaml` target fuzzes YAML 1.2 syntax and the pnpm 9.0 and Yarn Berry models (`D-071`, `D-072`), the `yarn` target the Yarn classic syntax and model (`D-072`), and the `workspace` target workspace patterns (`D-073`); CSS and the static evaluator follow when implemented. Planned for CI: short fuzzing runs on every pull request, and longer runs on a schedule. Crashing inputs become permanent regression fixtures. Fuzzing also checks the resource limits: no input may exceed configured depth, memory, or time bounds. The E1 budget is one CPU-hour of measured CPU time per target without findings (`D-044`). The first runs exposed and fixed exponential speculation in the TypeScript dialects. The desktop budget run of 2026-10-13 measured more than one CPU-hour per target with the processor-time helper (`D-046`) and found nothing on the final commit, after the token boundary contract had exposed span defects in the TypeScript dialects that are fixed (section 7 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).

### 3.4 Transformation fixtures

Each compatibility matrix entry in [MIGRATION_SPEC.md](MIGRATION_SPEC.md) has at least one small fixture project that isolates the feature. Each fixture contains:

- the source project (fictional, minimal, still a complete Next.js project);
- expected analysis results: unit statuses, findings with codes and locations;
- the expected plan (operations and manual tasks);
- the expected output tree (golden files);
- labels: the expected assessment of every unit, written by a human (section 3.6).

Composite fixtures combine many features into realistic fictional sites (portfolio, blog, documentation, small business), as described in [ROADMAP.md](ROADMAP.md).

### 3.5 Golden-file tests

- Expected outputs are stored next to each fixture and compared byte for byte.
- Golden files change only through an explicit update mode of the test runner, never automatically. The E2 fixtures follow this rule: their golden files change only through `frameshift-e2 update` (section 11 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). CLI golden files change only through `cargo test -p frameshift-cli --offline --locked --test cli -- --ignored update_golden_files` (section 12 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).
- Every golden change appears in the pull request diff and must be explained in the pull request description.
- A change to a rule's output requires a rule version increment (section 12 of [ARCHITECTURE.md](ARCHITECTURE.md)).
- Golden files include the plan document, the manifest, and the report (excluding the `run` section), not just generated source files.

### 3.6 Classification fixtures

Fixtures carry human-assigned labels for every unit: the status a careful expert would assign. Test runs compare Frameshift's classification with the labels:

- Frameshift says `auto` but the label is not `auto`: **false `auto`**, which fails the build unconditionally.
- Frameshift is more conservative than the label (`review`, `unsupported`, or `unknown` where the label says `auto`): tracked as **over-conservatism**. It does not fail the build, but feeds rule improvements.
- Mismatches between `unsupported` and `unknown`: reviewed case by case, because they affect user guidance.

### 3.7 Negative tests

- Every `U` entry in the compatibility matrix has a fixture proving that the construct is detected, reported with the correct code and location, preserved, and listed as a manual task.
- Malformed input: syntax errors, invalid encodings, truncated lockfiles, unsupported lockfile versions.
- Ambiguous rule matches: tests prove that overlapping rules are detected and fail closed (`FS4001`).
- Rule preconditions: for each rule, inputs that almost match but violate one precondition.

### 3.8 Regression tests

Every fixed defect gets a fixture or unit test named after the defect. Real-world evaluation findings become minimized fixtures, with fictional content replacing third-party code.

### 3.9 Determinism tests

- Every fixture runs twice with different degrees of parallelism. Plan, manifest, output tree, and report (excluding `run`) must be byte-identical.
- A test filesystem implementation returns directory entries in shuffled order. Results must not change. Implemented for discovery: the walker's unit tests feed 64 shuffled listings of one tree and require identical inventories and findings (`D-064`).
- The full fixture suite runs on Linux, Windows, and macOS, and output hashes are compared across platforms.
- Fixtures with CRLF line endings, byte order marks, non-ASCII paths, and names that differ only in Unicode normalization check that normalization rules hold.

### 3.10 Compatibility tests

- **Source versions:** fixtures exist in variants for the validated Next.js versions where behavior differs, for example synchronous `params` (Next.js 14) and Promise `params` (Next.js 15 and 16), or `middleware.ts` and `proxy.ts`.
- **Target profiles:** every fixture output builds with the pinned target profile. A target profile update is validated by rebuilding all fixtures before release.
- **Package managers:** lockfile parsing and output manifests are tested for npm, pnpm, Yarn classic, Yarn Berry, and Bun.
- **Knowledge base entries:** every `compatible` or `compat` package entry references at least one fixture that uses the package in an island or static component, builds it, and checks its behavior.

### 3.11 Build verification

Frameshift's CI installs dependencies for fixture outputs and runs the target's type check and production build with the pinned toolchain. This exercises the `build` verification level and checks that generated projects are valid. These fixtures are Frameshift's own, so running their builds in CI is safe. A dependency cache keeps runs stable and fast.

### 3.12 Behavioral comparisons

- **Static output comparison:** for statically exportable fixtures, CI builds the source with Next.js static export and the output with Astro, then compares the HTML per route after normalization. The same mechanism powers the `static-output` verification level. Normalization rules are decided by experiment E4 (`Q-005`). Expected scope:
  - Remove framework-specific markers: Astro island wrapper elements and attributes (comparing their contents), Next.js runtime payload scripts, and `data-nimg` attributes and default image styles.
  - Normalize whitespace according to HTML rendering rules, and normalize attribute order.
  - Compare the route set, title and meta tags, heading outline, landmarks, visible text, links, images (resolved source file, `alt`, dimensions), and form structure.
- **Runtime and browser-based island checks:** for island fixtures, a headless browser loads both builds, checks hydration, performs declared scripted interactions (for example "click the button, expect the text `Clicked 1 times`"), and compares resulting DOM text and selected accessibility-tree observations. M10 introduces the V1 product runtime level and exercises it on Frameshift's own fixtures. Product verification requires explicit execution authorization and a user-installed browser; the browser is never bundled. Browser tooling and the versioned check protocol are decided in M10.
- **Known blind spots:** CSS cascade, visual layout, animation, untested interactions, and general timing behavior are not proved by V1 verification. Focus or accessibility behavior counts as checked only where a declared runtime check exercises it. Every report lists remaining limits.

### 3.13 Safety and security tests

Each control in [SECURITY.md](SECURITY.md) has tests:

- The source tree is unchanged after every command (content hashes and file listing compared before and after). Implemented for discovery, comparing contents and link targets before and after two runs.
- No file is created outside the output and staging locations: the tests run with a watched temporary root.
- No external process starts without `--allow-exec`: the process runner is instrumented in tests.
- Symbolic links that escape the root are excluded and reported. Links inside the root are not processed twice. Implemented for discovery with links created at test time, for every link outcome on in-memory trees, and for `inspect` with links as configuration files, router directories, manifests, `src`, members, and the lockfile (`inspect-links`, `D-076`).
- Unsafe output paths (traversal, absolute paths, reserved names, control characters, case collisions) are rejected.
- Resource limits stop hostile inputs: deep nesting, very large files, very many files. Implemented for discovery's file, size, depth, entry, `.gitignore` byte, and matching work limits, including sparse files of 64 MiB created at test time, and for the parser's nesting limits.
- Source-derived strings are escaped in generated code, reports, and terminal output (injection fixtures). Implemented for `inspect`'s text and JSON output with the `inspect-hostile-names` fixture (`D-076`).
- `replace-managed` refuses to replace modified output, and never deletes data.
- Interrupted runs leave the source untouched, and the output absent or unchanged.

Fixtures that need symbolic links or very large files create them at test time instead of committing them.

### 3.14 Performance benchmarks

- A deterministic generator creates synthetic projects of fixed sizes (for example 100, 1,000, and 10,000 modules) with a realistic mix of routes, components, styles, and assets.
- Benchmarks measure wall time and peak memory for `inspect`, `analyze`, `plan`, and `migrate`, and parser throughput per core.
- CI records results as trends. Shared runners are too noisy for hard limits. Budgets are checked on reference hardware before each release.
- Pull requests that claim performance improvements include before-and-after measurements and profiles.

### 3.15 Cross-platform validation

The implemented M1 CI runs the native suite on Linux, Windows, and macOS, each on x86_64 and ARM64 (`D-051`, section 13 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). Earlier Windows/WSL evidence is retained in section 5. Platform-specific cases include native paths, actual long-path filesystem I/O, case-insensitive collisions, symlink permissions, executable bits, and line endings; many remain planned with their product subsystems. The path-value tests run on every host without using its filesystem interpretation. Release artifacts are smoke-tested on all six native runners (see [RELEASES.md](RELEASES.md)).

### 3.16 CLI end-to-end tests

The built binary runs against fixtures, and the tests assert standard output, standard error separation, JSON validity against the document schemas, exit codes for every condition in section 7 of [CLI.md](CLI.md), and behavior with `--format json`, `--quiet`, `NO_COLOR`, and non-terminal output.

### 3.17 Property tests

Invariants that must always hold are tested with generated inputs:

- Assessment shares and execution shares each sum to the total weight exactly.
- The file accounting invariant holds.
- Route readiness is consistent with unit statuses and the closure.
- Displayed percentages never exceed the exact value (truncation).
- Plans are invariant under input enumeration order.

## 4. Fixture corpus

- **Fictional only in the repository.** Committed fixtures contain only content written for Frameshift. No third-party project code is committed.
- **Organization (decided in `D-062`; the discovery cases exist):** product fixtures live below a root `fixtures/` directory, stored byte for byte (`fixtures/** -text`): `fixtures/discovery/<case>/` for `inspect`, discovery, and safety cases from M2, `fixtures/features/<feature-id>[--<variant>]/` with one compatibility matrix feature or `U` entry each, and `fixtures/composites/<site>/` for composite sites. Each fixture holds `fixture.json` (human-written metadata), `labels.json` (human-written expected unit statuses, absent in discovery cases), `source/` (the fictional project), and `expected/` (one golden file per produced document, such as `analysis.json` and `plan.json`, and `output/` with the complete output tree including `.frameshift/`). `AGENTS.md` points to this layout. The E2 fixtures under `tools/frameshift-e2/fixtures/` (`source/`, `expected/`, `provenance.txt`) are experiment fixtures, not that layout.
- **Golden and label rules (`D-062`):** no tool writes `fixture.json` or `labels.json`. `expected/` changes only through an explicitly ignored update test, as for the CLI goldens. The harness injects fixed run metadata, so golden documents stay complete and are compared byte for byte. Symbolic links, very large inputs, and manifests that name real framework packages are created at test time and described in `fixture.json`. `schemas/fixture.schema.json` describes the discovery, package, workspace, and inspect metadata; the `labels.json` schema arrives with the first fixture that has labels.
- **Discovery cases (implemented):** six cases in `fixtures/discovery/` cover built-in ignores, `.gitignore` semantics, canonical order, limits, very large files, and links. `expected/discovery.txt` holds a deterministic text rendering of every inventory entry and finding, which the `frameshift.inspection` document only counts, updated with `cargo test -p frameshift-engine --offline --locked --test discovery_fixtures -- --ignored update_discovery_goldens`. A case whose inputs the host cannot create (symbolic links on Windows without the privilege) is skipped and keeps its golden. Fixture sources contain `.gitignore` files, so files they ignore must be added with `git add -f` (section 22 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).
- **Package cases (implemented):** twelve cases of kind `package` in `fixtures/discovery/` cover npm workspaces with aliases and links, a Bun JSONC lockfile, a `packageManager` field against foreign lockfiles, ambiguous lockfiles, a hostile manifest, lockfile errors (`D-069`), pnpm peer-context snapshots, rejected YAML/lockfile structures, and YAML limits (`D-071`), and Yarn classic and Berry lockfiles with their rejections, generation mismatch, and staleness (`D-072`). `fixture.json` lists the package directories to load and may lower `package.max-lockfile-bytes`, `text.max-source-bytes`, and the JSON, YAML, and Yarn limits. `expected/package.txt` renders the package models, resolutions, and findings, updated with `cargo test -p frameshift-engine --offline --locked --test package_fixtures -- --ignored update_package_goldens`. Their manifests and lockfiles name only fictional packages, so advisory scanners of the public repository report nothing for them (section 26 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).
- **Workspace cases (implemented):** eight cases of kind `workspace` in `fixtures/discovery/` cover npm, pnpm, Yarn 1, Yarn 4, and Bun workspaces with their root lockfiles, and adversarial cases for releases that disagree, rejected patterns, an ignored member, and `workspace.max-patterns` (`D-073`). `fixture.json` may lower the four `workspace.*` limits. `expected/workspace.txt` renders the evaluated releases, members, resolutions through the root lockfile, siblings, and findings, updated with `cargo test -p frameshift-engine --offline --locked --test workspace_fixtures -- --ignored update_workspace_goldens`. Their members agree with what the reference releases list for the same trees (section 30 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).
- **Inspect cases (implemented):** seven cases of kind `inspect` in `fixtures/discovery/` cover a pnpm App Router project with an ignored npm lockfile, an npm workspace with a hybrid Next.js site, an inconclusive member, and a library, and a Next.js 13 Pages Router project without a lockfile and with a rejected configuration name (`D-075`), and adversarial cases for hostile names, links as structure, resource exhaustion within the default limits, and contradictory evidence (`D-076`). Inspect cases may create symbolic links; a host that cannot create them skips the case and keeps its goldens. They commit route and configuration files; every manifest and lockfile that names the real `next` package is a `createdAtTestTime` file, so no committed manifest names it (`Q-014`). `expected/inspection.json` is the `frameshift.inspection` document, updated with `cargo test -p frameshift-engine --offline --locked --test inspect_fixtures -- --ignored update_inspect_goldens`, and `expected/inspection.txt` the CLI text, updated with `cargo test -p frameshift-cli --offline --locked --test inspect -- --ignored update_inspect_text_goldens`; the CLI's JSON output is compared with the engine golden (section 32 of [IMPLEMENTATION.md](IMPLEMENTATION.md)).
- **Differential workspace tests (implemented):** an opt-in test compares each release's members with the listings that the real package managers recorded for generated fictional trees outside the repository, and another prints workspace and Next.js detection for project directories such as the approved real-world sample (sections 30 and 31 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). Next.js detection tests create manifests naming `next` at test time only, so no committed manifest names it (`Q-014`).
- **Metadata:** each fixture declares the features it covers, the source version variant, the expected verification level, and, for island fixtures, the scripted interactions.
- **Coverage tracking:** a check ensures that every compatibility matrix entry is covered by at least one fixture. Missing coverage fails CI.

## 5. Real-world corpus

The YAML syntax conformance suite is separate from project evaluation: keep the `yaml/yaml-test-suite` data checkout outside the repository with byte-exact line endings. The opt-in runner, pinned suite revision, and measured results are in IMPLEMENTATION.md section 27. It reads test data without running external code. Fictional YAML regression tests and the `yaml` fuzz target run through the public engine API.

- A private list of real-world Next.js projects (repository URL, pinned commit, license) is maintained for evaluation. Agents propose projects with a reason for each, and the project owner approves the list (`D-030`).
- Projects are fetched on demand into a cache outside the repository and are never committed.
- Only projects whose licenses permit this use are included. Results containing third-party code or non-public evaluation data stay outside public source, issues, logs, and CI artifacts, consistent with [SECURITY.md](SECURITY.md). Public repository visibility does not authorize publishing corpus contents.
- Evaluation runs `inspect`, `analyze`, `migrate`, and `verify` up to the highest feasible level. Results are aggregated into a private evaluation report for each release.
- Interesting failures are minimized into fictional regression fixtures (section 3.8).

## 6. Measuring migration accuracy

Accuracy is measured on two populations: labeled fixtures (where the ground truth is known) and the real-world corpus (where ground truth comes from human review).

| Measure | Definition | Target |
| --- | --- | --- |
| False `auto` count | Units classified `auto` that the label or a human reviewer says are not safely migratable, or whose output fails verification | Zero. Any occurrence blocks release. |
| Classification agreement | Confusion matrix of Frameshift status against label, per status | Tracked. Over-conservatism trends downward over time. |
| Golden exactness | Fixtures whose full output matches golden files byte for byte | 100% in CI |
| Build success rate | Migrated projects whose output passes the `build` level | 100% for supported composite fixtures. Tracked on the real-world corpus. |
| Static output match rate | Compared routes that match after normalization, out of all compared routes | 100% for supported composite fixtures. Tracked on the real-world corpus. |
| Island behavior match | Scripted interactions with identical results, out of all scripted interactions | 100% for island fixtures |
| Unknown rate | Weight of `unknown` units on the real-world corpus | Tracked. Each recurring unknown construct becomes a candidate rule or a documented limitation. |
| Human review sample | Randomly sampled `auto` and `review` units from real-world migrations, reviewed for correctness | No incorrect `auto`. `review` checklists judged accurate. |

The evaluation report records these measures for each release, so accuracy is tracked over time and regressions are visible.

## 7. CI tiers

Standard GitHub-hosted runner execution is free for public repositories; larger runners and artifact storage have separate billing rules. The existing six-target PR matrix stays enabled. The following tiers describe the broader planned verification coverage, not a reduction of current checks.

| Tier | When | Contents |
| --- | --- | --- |
| Pull request | Every push to a pull request | Formatting, linting, unit tests, parser fixtures, golden fixtures, classification and negative fixtures, in-process determinism checks, safety tests, CLI end-to-end tests, short fuzzing, Conventional Commit checks. Linux on every push. Windows and macOS when the pull request touches platform-sensitive code, and always before merge. |
| Scheduled | Nightly or weekly | Long fuzzing, full conformance suites, fixture builds, static output comparisons, browser island checks, benchmark trends, cross-platform determinism hash comparison, real-world corpus evaluation |
| Release | Release pull requests and tags | Everything in the scheduled tier, plus reproducibility checks and smoke tests of release artifacts on every platform with a native runner |

The implemented CI, Release, and Package workflows and their current limits are described in [RELEASES.md](RELEASES.md). The scheduled tiers below remain planned. `D-051` currently runs native tests and packaging on every pull request for all six targets rather than limiting Windows and macOS to platform-sensitive changes.

## 8. Open questions

- Normalization rules and thresholds for static output comparison, and the HTML parser to use (`Q-005`).
- Browser tool and versioned interaction-check protocol for V1 runtime verification and fixture checks (decided in M10).
- Concrete project list for the real-world corpus, proposed by agents and approved by the project owner in M5 (`D-030`).
