//! The compile-time registry of migrate v0's transformation rules (`D-104`).
//!
//! A rule's version increases whenever its output can change. Planning checks
//! every operation against this registry, so an operation can only name a
//! registered rule of its own kind, and one source file can only reach one
//! rule (ARCHITECTURE.md section 12.2).

use super::OperationKind;

/// One transformation rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// The identifier below the catalog's migration path, such as
    /// `route.static-page`.
    pub id: &'static str,
    /// Increases whenever the rule's output can change.
    pub version: u64,
    /// The operation that the rule produces.
    pub kind: OperationKind,
    /// One sentence that says what the rule does.
    pub summary: &'static str,
}

const fn rule(id: &'static str, kind: OperationKind, summary: &'static str) -> Rule {
    Rule {
        id,
        version: 1,
        kind,
        summary,
    }
}

/// Every registered rule, sorted by identifier.
pub const RULES: &[Rule] = &[
    rule(
        "asset.verbatim",
        OperationKind::Copy,
        "Copies a public asset or non-code data file unchanged.",
    ),
    rule(
        "component.astro",
        OperationKind::Create,
        "Converts a server component into an Astro component.",
    ),
    rule(
        "component.preact-static",
        OperationKind::Transform,
        "Keeps a component as a script for Preact and rewrites its imports.",
    ),
    rule(
        "config.astro",
        OperationKind::Config,
        "Generates astro.config.mjs with the Preact integration.",
    ),
    rule(
        "config.gitignore",
        OperationKind::Config,
        "Keeps the source .gitignore entries and adds Astro's build directories.",
    ),
    rule(
        "config.package-manifest",
        OperationKind::Config,
        "Generates package.json without Next.js and React, with the pinned target profile.",
    ),
    rule(
        "config.tsconfig",
        OperationKind::Config,
        "Generates tsconfig.json with Astro's strict settings and Preact's JSX settings.",
    ),
    rule(
        "layout.chain",
        OperationKind::Create,
        "Converts a layout into an Astro layout that wraps its parent.",
    ),
    rule(
        "metadata.favicon",
        OperationKind::Copy,
        "Copies the root favicon to public/.",
    ),
    rule(
        "module.relocate",
        OperationKind::Transform,
        "Moves a module or declaration file and rewrites its local specifiers.",
    ),
    rule(
        "preserve.unmigrated",
        OperationKind::Preserve,
        "Copies an unmigrated file below .frameshift/unmigrated/ and adds a manual task.",
    ),
    rule(
        "report.manifest",
        OperationKind::Config,
        "Generates .frameshift/manifest.json with the versions, plan hash, and file digests.",
    ),
    rule(
        "report.summary",
        OperationKind::Config,
        "Generates .frameshift/SUMMARY.md with the results and manual tasks.",
    ),
    rule(
        "route.static-page",
        OperationKind::Create,
        "Converts a static page into an Astro page inside its nearest layout.",
    ),
    rule(
        "style.relocate",
        OperationKind::Copy,
        "Moves a stylesheet whose references stay valid at its new location.",
    ),
];

/// Looks up a rule by its identifier below the catalog's migration path.
pub fn find(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.id == id)
}
