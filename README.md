# Frameshift

Frameshift is a framework migration engine in early development with a command-line interface. It will analyze existing software projects and migrate them from one framework to another, preserve their intended behavior, and report what it migrated, what it could not migrate, and why.

> **Status: M0 complete, M1 in progress, M2 started.** The repository contains a buildable Rust workspace with tested immutable source-text primitives, parser-directed JavaScript scanners, complete regular-expression literal validation, ECMAScript identifier classification from pinned Unicode 18.0.0 tables, character reference decoding with pinned HTML and XHTML tables, a JavaScript parser with an arena tree for complete Script and Module source text, including sloppy mode and the Annex B web-compatibility grammar, an explicitly selected JSX dialect, and explicitly selected TypeScript, TSX, and declaration-file dialects with structured type nodes, declarations, and TypeScript extensions, a deterministic span edit core with provenance, an Astro component printer that converts React JSX and maps every output byte to its source, and a development-only harness that measures parser throughput and memory on generated JavaScript, JSX, and TypeScript corpora and on real files outside the repository, with a reproducible contract-check mode for the generated corpora, a development-only coverage-guided fuzzer for the parsers, scanners, character reference decoders, and edit core, and a development-only Go helper that measures the processor time of fuzzing runs. On an approved sample of 13,292 files from twelve open-source Next.js projects, 99.92 percent parse and every rejection is explained. The parser experiment (E1) passed. The rewriting and Astro printing experiment (E2) passed as well: three fictional Next.js fixtures are migrated deterministically, byte for byte identical on Windows and Linux, and Astro builds the results. A CLI shell now provides help and version information, including versioned JSON output (`D-050`). Native CI, cross-platform E2 hash comparison, Release Please PR automation, and deterministic archive packaging exist (`D-051`, `D-052`). All M1 foundation contracts are implemented, including deterministic collections and a checked set of resource limits (`D-060`, `D-061`). Publication is gated until M1 is accepted under the criteria of `D-063`. M2 has started: the engine discovers a project's files read-only, follows no links, never opens environment files, honors `.gitignore` files with Git's semantics, and reports every skipped entry and reached limit (`D-064`, `D-065`). Release PR maintenance is temporarily paused for the environment-secret transition (`D-066`). There is no `inspect` or migration command yet. See [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) for existing code, contracts, limits, and development commands. The product commands below remain planned.

## The problem

Moving a project to a different framework is expensive and risky. Teams rewrite by hand, write one-off codemods, or use tools that produce output without saying what they missed. The result is often a migration that compiles but quietly behaves differently.

Many websites built with Next.js are a concrete example. They are mostly static and have no real need for a server-oriented React framework, yet they ship a full client-side React runtime to every visitor. Astro with Preact islands would serve the same content with far less JavaScript, but migrating by hand is tedious and error-prone.

Frameshift aims to make such migrations analyzable, deterministic, and explainable. A developer should know how much of a project can be migrated before anything changes. They should be able to inspect the migration plan, and receive a result whose remaining gaps are listed precisely.

## Planned capabilities

- Analyze a repository: framework, version, project structure, package manager, dependencies, and lockfiles.
- Discover routes, layouts, components, configuration, assets, styles, and relevant application logic, and build a structured model of the project.
- Classify every construct as automatically migratable, migratable with review, unsupported, or unknown, and explain each classification.
- Produce an inspectable migration plan before writing anything.
- Report feasibility, migration completion, and verification with precisely defined, reproducible percentages.
- Apply supported transformations deterministically and generate the target project in a separate directory, without touching the source.
- Verify results: structural checks, builds, and comparison of the generated pages.
- Produce human-readable and JSON reports that list every incomplete, unsupported, or potentially unsafe transformation.

Frameshift will never present an incomplete migration as a success, and it never treats a high percentage as proof of identical behavior.

## First migration path: Next.js to Astro + Preact

The first supported path targets Next.js App Router projects that are mostly static. Static components become Astro components that ship no JavaScript. Interactive components become Preact islands. Routes, content, appearance, accessibility, and behavior are preserved, and the output is deployable as a static site. Request-time server features such as middleware, Server Actions, and cookies are detected and explained, not migrated. See [docs/MIGRATION_SPEC.md](docs/MIGRATION_SPEC.md).

Runtime dependencies are reviewed explicitly. The SHA-256 dependency is pinned to RustCrypto `sha2` 0.11.1 with default features disabled; its exact transitive graph, third-party notices, and development-only advisory checks are recorded under `D-058` in [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md). On a fresh machine, prepare the dependency cache with `cargo fetch --locked` before the documented offline build and test commands. Product runtime remains offline.

## Development stages

| Stage | Theme |
| --- | --- |
| V1 | Next.js to Astro migration MVP: a production-quality CLI for the first migration path, with honest reporting and verification |
| V2 | Extensible engine: framework-independent intermediate representation, source and target adapters, declarative recipes, more migration paths |
| V3 | Universal platform: substantially different frameworks, server-side features, advanced verification, and integrations, within documented limits |

Milestones are defined by capabilities, not dates. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Engineering principles

- **Correctness and honesty first.** Anything Frameshift cannot handle with certainty is reported as unknown or unsupported.
- **Deterministic and offline.** The same input produces the same output. No network access and no AI service in normal operation.
- **Safe by design.** Source repositories are treated as untrusted. They are never modified, and their code is never executed without explicit authorization.
- **Explainable.** Every decision traces back to a rule, a source location, and a documented reason.
- **Minimal dependencies.** Purpose-built components where external libraries do not fit, with understandable structures and documented tradeoffs.
- **Forward-looking architecture.** Every increment anticipates the documented V1 to V3 evolution, including subsystem boundaries, extensibility, and compatibility (`D-033`).
- **CLI-first, engine-centered.** The migration engine is independent of the CLI, so future interfaces can reuse it.
- **Performance-aware.** Analysis of typical projects should be near-instantaneous.

## Available CLI shell

The engine foundation also exposes strict canonical `SourcePath` values and separately validated portable `OutputPath` values through `frameshift_engine::foundation` (`D-053`, `D-054`). They preserve source spelling and reject ambiguous structure without normalization. Stable `UnitId` values distinguish file, URL-pattern, and configuration subjects with unambiguous percent encoding (`D-056`, IMPLEMENTATION.md section 16). `FindingCode`, `RuleId`, and `FeatureId` values validate catalog spelling, independently of whether a catalog entry ships (`D-055`, IMPLEMENTATION.md section 15). Bounded finding records add project, file, and validated text scopes and deterministic report-local numbering (`D-057`, IMPLEMENTATION.md section 17). The foundation also hashes exact bytes with bounded SHA-256 streaming and canonical `sha256:<64 lowercase hex>` values (`D-058`, `D-059`, IMPLEMENTATION.md sections 18 and 19). `KeyTable` assigns dense identities from sorted keys and `DenseSlots` collects results independently of completion order (`D-060`); `RunLimits`, `Budget`, and the engine's `limits::Limits` make every resource limit explicit and checked (`D-061`, IMPLEMENTATION.md sections 20 and 21). These in-memory types do not inspect a filesystem or establish safe write locations. See section 14 of [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md). Read-only discovery and `.gitignore` matching are exposed through `frameshift_engine::input` (sections 22 and 23).

```text
cargo run -p frameshift-cli --offline --locked -- help
cargo run -p frameshift-cli --offline --locked -- version
cargo run -p frameshift-cli --offline --locked -- version --format json
```

The executable is `target/debug/frameshift` (`frameshift.exe` on Windows) after a workspace build. Its current version is the unreleased Cargo version `0.0.0`, not a published release. Help lists only available commands. Version output reports the actual build compiler, empty migration catalogs, and the explicitly embedded build commit (unknown in ordinary local builds).

## Planned command-line interface

The following commands illustrate the intended interface. They are a specification, not working software:

```text
frameshift inspect ./my-project
frameshift analyze ./my-project --to astro-preact
frameshift plan ./my-project --to astro-preact --out plan.json
frameshift migrate ./my-project --to astro-preact --out ../my-project-astro
frameshift verify ../my-project-astro --level build --allow-exec
```

See [docs/CLI.md](docs/CLI.md) for the full specification.

## Documentation

| Document | Contents |
| --- | --- |
| [AGENTS.md](AGENTS.md) | Binding instructions for all agents working on this repository |
| [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) | Existing code, text contracts, limits, development commands, and E1 evidence |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Pipeline, subsystems, project model, rule engine, planning, safe execution, verification, limits |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Stages, milestones, dependencies, acceptance criteria, risks, non-goals |
| [docs/MIGRATION_SPEC.md](docs/MIGRATION_SPEC.md) | Next.js to Astro + Preact: strategies, compatibility matrix, examples |
| [docs/MIGRATION_REPORTING.md](docs/MIGRATION_REPORTING.md) | Status model, metric definitions, weights, report formats, sample reports |
| [docs/CLI.md](docs/CLI.md) | Commands, options, input and output handling, exit codes |
| [docs/ENGINEERING.md](docs/ENGINEERING.md) | Implementation language evaluation, build-versus-adopt policy, performance, conventions |
| [docs/TESTING.md](docs/TESTING.md) | Test strategy and measurement of migration accuracy |
| [docs/SECURITY.md](docs/SECURITY.md) | Threat model, safety controls, privacy, publication preparation |
| [docs/RELEASES.md](docs/RELEASES.md) | Conventional Commits, Release Please, CI/CD, artifacts, signing |
| [docs/LLM_POLICY.md](docs/LLM_POLICY.md) | Deterministic-first policy and the future role of optional model assistance |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Accepted, proposed, and deferred decisions, and open questions |

## Project status

- **Implementation:** started with the M0 foundation. The help and version CLI shell exists; migration commands remain planned. Rust is the primary implementation language. Other languages may be used for specific, important tasks where they offer a demonstrable quality advantage (see [docs/DECISIONS.md](docs/DECISIONS.md)).
- **Repository:** `eiserv/frameshift` on GitHub. The repository is public under `D-066`; this does not mark M1 complete or publish a release.
- **License:** GNU Affero General Public License, version 3 or later (`AGPL-3.0-or-later`). See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- **Contributions:** external code contributions are not being accepted yet. See [CONTRIBUTING.md](CONTRIBUTING.md).
- **Security:** report suspected vulnerabilities privately through [SECURITY.md](SECURITY.md).

## License and corresponding source

Copyright (C) 2026 Frameshift contributors.

Frameshift is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. It is distributed without any warranty, including the implied warranties of merchantability or fitness for a particular purpose. See [LICENSE](LICENSE) for the complete terms. Third-party materials retain the notices and terms in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Corresponding source, build scripts, and pinned toolchain configuration are in this repository. For a distributed binary, use the exact `buildCommit` printed by `frameshift version --format json`, available through `https://github.com/eiserv/frameshift/tree/<buildCommit>`. Public binary distribution requires that source commit to be publicly accessible. Frameshift's normal operation has no network access or telemetry.
