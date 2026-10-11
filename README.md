# Frameshift

Frameshift is a framework migration engine in early development with a command-line interface. It will analyze existing software projects and migrate them from one framework to another, preserve their intended behavior, and report what it migrated, what it could not migrate, and why.

> **Status: initial M0 implementation.** The repository now contains a buildable Rust workspace with tested immutable source-text primitives. The parser and rewriting experiments remain open. There is no CLI or working migration command yet. See [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) for existing code, contracts, limits, and development commands. The product commands below remain planned.

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
- **Minimal dependencies.** Purpose-built components where external libraries do not fit, within KISS and YAGNI.
- **CLI-first, engine-centered.** The migration engine is independent of the CLI, so future interfaces can reuse it.
- **Performance-aware.** Analysis of typical projects should be near-instantaneous.

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
| [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) | Existing code, text contracts, limits, and development commands |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Pipeline, subsystems, project model, rule engine, planning, safe execution, verification, limits |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Stages, milestones, dependencies, acceptance criteria, risks, non-goals |
| [docs/MIGRATION_SPEC.md](docs/MIGRATION_SPEC.md) | Next.js to Astro + Preact: strategies, compatibility matrix, examples |
| [docs/MIGRATION_REPORTING.md](docs/MIGRATION_REPORTING.md) | Status model, metric definitions, weights, report formats, sample reports |
| [docs/CLI.md](docs/CLI.md) | Commands, options, input and output handling, exit codes |
| [docs/ENGINEERING.md](docs/ENGINEERING.md) | Implementation language evaluation, build-versus-adopt policy, performance, conventions |
| [docs/TESTING.md](docs/TESTING.md) | Test strategy and measurement of migration accuracy |
| [docs/SECURITY.md](docs/SECURITY.md) | Threat model, safety controls, privacy, private phase rules |
| [docs/RELEASES.md](docs/RELEASES.md) | Conventional Commits, Release Please, CI/CD, artifacts, signing |
| [docs/LLM_POLICY.md](docs/LLM_POLICY.md) | Deterministic-first policy and the future role of optional model assistance |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Accepted, proposed, and deferred decisions, and open questions |

## Project status

- **Implementation:** started with the M0 foundation. No product command exists yet. Rust is the primary implementation language. Other languages may be used for specific, important tasks where they offer a demonstrable quality advantage (see [docs/DECISIONS.md](docs/DECISIONS.md)).
- **Repository:** private (`eiserv/frameshift` on GitHub). No license has been chosen yet, so no rights are granted beyond those of the copyright holder.
- **Contributions:** not open during the private phase.
