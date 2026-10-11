# Agent Guidance

These instructions are binding for every agent working on Frameshift, whatever the tool or vendor.

- **Before starting any work,** read this file completely, then work through the documentation in `docs/` and look for everything relevant to your task: specifications, decisions, conventions, limits, and open questions. Do not start from assumptions when the documentation answers the question. If documents contradict each other or the code, report it and resolve it rather than picking one silently.
- **Keep the documentation and this file current at all times.** Whenever a change affects anything described in `docs/`, `README.md`, or `AGENTS.md` (behavior, commands, architecture, conventions, decisions, limits, file locations), update the affected documents in the same change. Outdated documentation counts as a defect.
- **At the end of every run,** check whether your changes made any statement here or in `docs/` outdated, or whether information important for future work is missing, and fix it in the same run.
- Record only what is true for the repository as it is. Never describe planned code, files, or commands as if they already exist. Keep this file compact.

## Orientation

- **Identity.** The project is named Frameshift, with exactly this capitalization. The CLI executable and command is `frameshift`. Do not introduce other names, temporary branding, or package identities. Naming conventions are in `docs/ENGINEERING.md` (section 8).
- **Purpose.** A deterministic, rule-based engine that analyzes software projects and migrates them between frameworks while preserving behavior, with transparent reporting. The first migration path is Next.js (App Router) to Astro with Preact islands, as static output (`docs/MIGRATION_SPEC.md`). The product is a CLI. The engine is independent of it.
- **Current state: planning, documentation only.** There is no source code, toolchain, package manifest, build system, test suite, CI workflow, release configuration, or working command. Everything the documentation says about commands, modules, files, workflows, and behavior is a specification or proposal. The remote `origin` is the private repository `eiserv/frameshift` (`D-026`), default branch `main`. Work on branches and integrate through pull requests.
- **Implementation language.** Rust is the primary language for engine and CLI (`D-018`). Other languages are allowed only for specific, important tasks with a demonstrable quality advantage, a decision record, and the requirements in `docs/ENGINEERING.md` section 2.7 (`D-019`). Effort and time are not arguments against quality (`D-020`). No toolchain is initialized yet: do not initialize one, create package manifests, or write implementation code unless the task explicitly asks for it.
- **Documentation map.**
  - `README.md`: overview and status.
  - `docs/ARCHITECTURE.md`: pipeline, subsystem layers, project model, rule engine, planning, safe execution, verification levels, limits.
  - `docs/ROADMAP.md`: stages V1 to V3, milestones M0 to M24 and V3 tracks, acceptance criteria.
  - `docs/MIGRATION_SPEC.md`: the Next.js to Astro + Preact path and its compatibility matrix.
  - `docs/MIGRATION_REPORTING.md`: status model, metrics, weights, report formats, finding codes.
  - `docs/CLI.md`: planned commands, options, exit codes.
  - `docs/ENGINEERING.md`: language evaluation, build-versus-adopt policy, performance budgets, determinism rules, conventions.
  - `docs/TESTING.md`, `docs/SECURITY.md`, `docs/RELEASES.md`, `docs/LLM_POLICY.md`.
  - `docs/DECISIONS.md`: the decision register. Only "Accepted" entries are binding. Proposals are recommendations.
- **Architectural invariants (binding for future implementation).**
  - All migration logic lives in the engine. The CLI only parses arguments, calls the engine, renders results, and maps exit codes.
  - Analysis is read-only, planning is a pure function, execution writes only to staging and the output location, and verification runs external tools only with explicit authorization.
  - Source repositories are untrusted. Never modify them, never execute their code without authorization, never write outside the output location.
  - Fail closed. Anything not positively handled by a rule is `unknown` or `unsupported`, never reported as migrated. No percentage claims behavioral equivalence.
  - Determinism. The same input, options, and versions produce byte-identical plans, output, and reports, except for the report's `run` section.
  - No network access and no LLM in normal operation.
- **Commands.** None exist yet. Add verified build, test, lint, and run commands here once they exist.

## Working Practices

- **Language.** English is mandatory for every repository artifact: documentation, code, comments, issues, pull requests, reviews, commit messages, changelogs, release notes, error messages, CLI output, configuration descriptions, decision records, and future UI text. The only exception is direct chat with the user, which uses the user's language. A German chat gets German replies, but files stay English.
- **Writing style.**
  - Clear, precise, professional English.
  - Never use em dashes (Unicode U+2014). Use commas, colons, semicolons, parentheses, or separate sentences instead.
  - No marketing language or vague claims. Explain important decisions and their tradeoffs.
  - Distinguish confirmed facts from proposals, and label unresolved decisions.
  - Keep documents useful rather than long.
- **KISS and YAGNI.** Solve the concrete requirement of the current stage with the simplest understandable structure. No speculative abstractions, options, or extension points.
- **Deterministic correctness.** Follow the determinism rules (`docs/ENGINEERING.md`, section 6) and the fail-closed rules (section 7.3). Unexpected input must surface as a finding or an internal error, never as silent success.
- **Dependencies.** Minimize them. Every runtime dependency, and every tool that affects build output, needs an accepted entry in `docs/DECISIONS.md` that is evaluated against `docs/ENGINEERING.md` (section 3). Prefer purpose-built code when a library does not fit, within KISS and YAGNI.
- **Performance.** Respect the budgets in `docs/ENGINEERING.md` (section 4). Avoid quadratic algorithms over files or nodes. Measure before optimizing.
- **Repository safety.** Never run code from analyzed projects in tools or tests. Commit only fictional fixtures, never third-party project code.
- **No UI work.** Frameshift is CLI-only (`D-004`). Do not build GUI, web, desktop, or terminal UI frontends unless the decision register changes.
- **Documentation synchronization.** Mandatory, see the rules at the top of this file. When implementing something specified, change the specification to describe actual behavior, and keep marking everything else as planned. Record decisions in `docs/DECISIONS.md`.
- **Conventional Commits.** Commit messages and pull request titles follow `<type>(<optional scope>): <description>`, for example `feat(analyzer): detect static route dependencies`.
  - Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `style`, `chore`, `revert`.
  - Mark breaking changes with `!` after the type or scope, or with a `BREAKING CHANGE:` footer.
  - Release Please derives versions and changelogs from these messages (`docs/RELEASES.md`).
- **Small commits.** As soon as a step stands on its own (a fix with its test, a rule with its fixtures, a documentation update), commit it with its own message. Do not collect a large change into one commit. Each commit should pass the relevant checks.
- **Pull requests and squash merges.**
  - Changes reach `main` only through pull requests, merged by squash. Never push directly to `main`.
  - The squash commit uses the pull request title, so the title must be a valid Conventional Commit, including `!` for breaking changes.
  - Keep unrelated changes in separate branches and pull requests.
  - Push branches, open pull requests, or create issues only when the task calls for it or the user authorizes it.
- **Tests and validation.** Add proportionate tests that cover behavior and error boundaries, and a regression test with every fix. Run the relevant checks before committing once they exist. For documentation-only changes, check links and the absence of em dashes.
- **Releases and CI/CD.** Versions change only through Release Please. Never edit version numbers by hand, and never create tags or releases manually. The workflows in `docs/RELEASES.md` are planned. None exist yet.
- **Private repository.** Do not publish the repository, change its visibility, create mirrors, upload its contents or analyzed code to external services, add telemetry, or assume a license. Release artifacts stay private (`D-012`, `docs/SECURITY.md` section 10).
- **Maintaining these instructions.** Keep this file compact. When real code exists, add entries for actual modules, invariants, and verified commands, and remove statements that become false. Do not add speculative paths or implementation details.
