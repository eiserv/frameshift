# Testing and Verification Strategy

**Status: strategy with M0 contract tests.** Six source-text, ten public-engine rewriting, eleven public-engine trivia/string contract tests, eight public-engine numeric contract tests, eight public-engine punctuation contract tests, and ten public-engine template contract tests run with Cargo on Linux x86_64. The preceding 43-test suite was validated on Windows x86_64; the template increment awaits Windows and macOS validation. They cover byte preservation, Unicode and newline positions, invalid spans, UTF-8 errors, size limits, edit conflicts, complete source accounting, byte provenance, permutation-invariant results, comment boundaries, strict strings, escape errors, numeric spelling categories, separator and exponent boundaries, strict leading-zero rejection, conservative Unicode numeric boundaries, lexical limits, maximal punctuation matches, explicit lexical goals, optional-chain lookahead, type-closing rescans, template segment boundaries, tagged/untagged escape handling, source-spelling preservation, and long code-point escapes. Bounded generated lexical tests check deterministic errors and valid spans; they are not coverage-guided fuzzing or full parser conformance. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for commands and actual test locations. Full parser, migration, golden, fuzzing, CI, and cross-platform test infrastructure remain planned. This document defines how Frameshift itself will be tested and how the accuracy of its migrations will be measured. Verifying a user's migrated project (`frameshift verify`) is a product feature, defined in section 16 of [ARCHITECTURE.md](ARCHITECTURE.md). This document covers testing Frameshift and uses those verification levels where relevant.

## 1. Goals

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

Pure functions and small components: path normalization, semver matching, gitignore matching, the static evaluator, metric arithmetic, the edit engine, and individual rule preconditions. Fast, exhaustive where the input space is small.

### 3.2 Parser fixtures and conformance

- **Fixture files** per syntax area (modules, expressions, JSX, TypeScript types, error cases), with expected tree snapshots and span checks. For every node, slicing the original text by the node's span must yield exactly that construct.
- **Conformance suites:** public test suites with compatible licenses, such as the TypeScript compiler's conformance tests (Apache-2.0) and test262, serve as parser inputs. Expected outcome: files the reference implementation accepts must parse, and files it rejects must not produce a tree that the engine would transform. These suites are fetched or vendored with license notices, and are never shipped.
- **Round-trip property:** applying an empty edit set reproduces the input byte for byte.
- The same applies to the CSS, JSONC, YAML subset, and lockfile parsers, each against its specification.
- **Future recovery tests (`D-034`):** multiple errors in source order, bounded diagnostic and recovery work, forward progress, refusal to synchronize in an unknown lexical context, and exclusion of the entire erroneous file from transformations even when a partial tree is recovered.

### 3.3 Fuzzing

Every parser of untrusted input and the static evaluator are fuzzed. Short fuzzing runs on every pull request, and longer runs on a schedule. Crashing inputs become permanent regression fixtures. Fuzzing also checks the resource limits: no input may exceed configured depth, memory, or time bounds.

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
- Golden files change only through an explicit update mode of the test runner, never automatically.
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
- A test filesystem implementation returns directory entries in shuffled order. Results must not change.
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

- The source tree is unchanged after every command (content hashes and file listing compared before and after).
- No file is created outside the output and staging locations: the tests run with a watched temporary root.
- No external process starts without `--allow-exec`: the process runner is instrumented in tests.
- Symbolic links that escape the root are excluded and reported. Links inside the root are not processed twice.
- Unsafe output paths (traversal, absolute paths, reserved names, control characters, case collisions) are rejected.
- Resource limits stop hostile inputs: deep nesting, very large files, very many files.
- Source-derived strings are escaped in generated code, reports, and terminal output (injection fixtures).
- `replace-managed` refuses to replace modified output, and never deletes data.
- Interrupted runs leave the source untouched, and the output absent or unchanged.

Fixtures that need symbolic links or very large files create them at test time instead of committing them.

### 3.14 Performance benchmarks

- A deterministic generator creates synthetic projects of fixed sizes (for example 100, 1,000, and 10,000 modules) with a realistic mix of routes, components, styles, and assets.
- Benchmarks measure wall time and peak memory for `inspect`, `analyze`, `plan`, and `migrate`, and parser throughput per core.
- CI records results as trends. Shared runners are too noisy for hard limits. Budgets are checked on reference hardware before each release.
- Pull requests that claim performance improvements include before-and-after measurements and profiles.

### 3.15 Cross-platform validation

The test suite runs on Linux, Windows, and macOS, and on ARM64 where runners are available. Platform-specific cases: Windows paths and reserved names, long paths, case-insensitive filesystems, symbolic link permissions, executable bits, and line endings. Release artifacts are smoke-tested on each platform where a native runner exists (see [RELEASES.md](RELEASES.md)).

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
- **Organization (planned):** one directory per fixture, holding the source project, expected results, labels, and golden output. Single-feature fixtures are named after their feature IDs. Composite fixtures have descriptive names. The concrete layout is decided in M1 and recorded in `AGENTS.md` once it exists.
- **Metadata:** each fixture declares the features it covers, the source version variant, the expected verification level, and, for island fixtures, the scripted interactions.
- **Coverage tracking:** a check ensures that every compatibility matrix entry is covered by at least one fixture. Missing coverage fails CI.

## 5. Real-world corpus

- A private list of real-world Next.js projects (repository URL, pinned commit, license) is maintained for evaluation. Agents propose projects with a reason for each, and the project owner approves the list (`D-030`).
- Projects are fetched on demand into a cache outside the repository and are never committed.
- Only projects whose licenses permit this use are included. During the private phase, no results containing third-party code leave the development environment or CI, consistent with [SECURITY.md](SECURITY.md).
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

GitHub Actions minutes for private repositories are limited, and macOS minutes cost more. Tiers keep pull request feedback fast while still covering everything before release.

| Tier | When | Contents |
| --- | --- | --- |
| Pull request | Every push to a pull request | Formatting, linting, unit tests, parser fixtures, golden fixtures, classification and negative fixtures, in-process determinism checks, safety tests, CLI end-to-end tests, short fuzzing, Conventional Commit checks. Linux on every push. Windows and macOS when the pull request touches platform-sensitive code, and always before merge. |
| Scheduled | Nightly or weekly | Long fuzzing, full conformance suites, fixture builds, static output comparisons, browser island checks, benchmark trends, cross-platform determinism hash comparison, real-world corpus evaluation |
| Release | Release pull requests and tags | Everything in the scheduled tier, plus reproducibility checks and smoke tests of release artifacts on every platform with a native runner |

The planned workflows are described in [RELEASES.md](RELEASES.md). None exist yet.

## 8. Open questions

- Normalization rules and thresholds for static output comparison, and the HTML parser to use (`Q-005`).
- Browser tool and versioned interaction-check protocol for V1 runtime verification and fixture checks (decided in M10).
- Concrete project list for the real-world corpus, proposed by agents and approved by the project owner in M5 (`D-030`).
