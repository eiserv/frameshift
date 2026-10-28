//! Generated project files of migrate v0 for the pinned target profile
//! `astro7-preact10` (`D-101`), and the human-readable summary.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use frameshift_core::SourceText;
use frameshift_emit::LineEnding;
use frameshift_package::PackageManifest;

use super::{ManualTask, Operation, OperationKind, RULESET, SUMMARY, TaskKind, UNMIGRATED};

const ASTRO_VERSION: &str = "7.3.8";
const ASTRO_PREACT_VERSION: &str = "6.0.6";
const PREACT_VERSION: &str = "10.29.8";
const PREACT_COMPAT_VERSION: &str = "18.3.2";

/// Output paths reserved for generated files.
pub(crate) const GENERATED: &[&str] = &[
    "package.json",
    "astro.config.mjs",
    "tsconfig.json",
    ".gitignore",
    SUMMARY,
];

pub(crate) struct Generated {
    pub(crate) files: Vec<(&'static str, &'static str, Vec<u8>)>,
    pub(crate) removed: Vec<String>,
}

// Next.js, React, and their tooling have no role in the Astro project;
// React is aliased to `preact/compat` instead.
fn removed(name: &str) -> bool {
    matches!(
        name,
        "next"
            | "react"
            | "react-dom"
            | "eslint-config-next"
            | "@types/react"
            | "@types/react-dom"
            | "server-only"
            | "client-only"
    ) || name.starts_with("@next/")
}

pub(crate) fn generate(
    manifest: &PackageManifest,
    gitignore: Option<&SourceText>,
    line_ending: LineEnding,
) -> Generated {
    let eol = match line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
    };
    let mut removed_names = Vec::new();
    let mut dependencies = BTreeMap::new();
    for entry in &manifest.dependencies {
        if removed(&entry.key) {
            removed_names.push(entry.key.to_string());
        } else {
            dependencies.insert(entry.key.to_string(), entry.value.text.to_string());
        }
    }
    let compat = format!("npm:@preact/compat@{PREACT_COMPAT_VERSION}");
    for (name, version) in [
        ("astro", ASTRO_VERSION.to_owned()),
        ("@astrojs/preact", ASTRO_PREACT_VERSION.to_owned()),
        ("preact", PREACT_VERSION.to_owned()),
        ("react", compat.clone()),
        ("react-dom", compat),
    ] {
        dependencies.insert(name.to_owned(), version);
    }
    let mut dev = BTreeMap::new();
    for entry in &manifest.dev_dependencies {
        if removed(&entry.key) {
            removed_names.push(entry.key.to_string());
        } else if !dependencies.contains_key(&*entry.key) {
            dev.insert(entry.key.to_string(), entry.value.text.to_string());
        }
    }
    removed_names.sort();
    removed_names.dedup();
    let mut package = String::from("{");
    package.push_str(eol);
    if let Some(name) = &manifest.name {
        let _ = write!(package, "  \"name\": {},{eol}", json_string(&name.value));
    }
    for line in [
        "  \"private\": true,",
        "  \"type\": \"module\",",
        "  \"scripts\": {",
        "    \"dev\": \"astro dev\",",
        "    \"build\": \"astro build\",",
        "    \"preview\": \"astro preview\"",
        "  },",
    ] {
        package.push_str(line);
        package.push_str(eol);
    }
    let object = |out: &mut String, name: &str, members: &BTreeMap<String, String>, last: bool| {
        let _ = write!(out, "  \"{name}\": {{{eol}");
        let lines: Vec<String> = members
            .iter()
            .map(|(key, value)| format!("    {}: {}", json_string(key), json_string(value)))
            .collect();
        out.push_str(&lines.join(&format!(",{eol}")));
        let _ = write!(out, "{eol}  }}{}{eol}", if last { "" } else { "," });
    };
    object(&mut package, "dependencies", &dependencies, dev.is_empty());
    if !dev.is_empty() {
        object(&mut package, "devDependencies", &dev, true);
    }
    package.push('}');
    package.push_str(eol);
    let config = [
        "import { defineConfig } from 'astro/config';",
        "import preact from '@astrojs/preact';",
        "",
        "export default defineConfig({",
        "  integrations: [preact({ compat: true })],",
        "});",
        "",
    ]
    .join(eol);
    let tsconfig = [
        "{",
        "  \"extends\": \"astro/tsconfigs/strict\",",
        "  \"include\": [\".astro/types.d.ts\", \"**/*\"],",
        "  \"exclude\": [\"dist\"],",
        "  \"compilerOptions\": {",
        "    \"jsx\": \"react-jsx\",",
        "    \"jsxImportSource\": \"preact\"",
        "  }",
        "}",
        "",
    ]
    .join(eol);
    let mut ignore = gitignore.map_or_else(
        || format!("node_modules/{eol}"),
        |text| text.as_str().to_owned(),
    );
    let existing: Vec<&str> = ignore.lines().map(str::trim).collect();
    let missing: Vec<&str> = ["dist/", ".astro/"]
        .into_iter()
        .filter(|entry| !existing.contains(entry))
        .collect();
    if !missing.is_empty() {
        if !ignore.is_empty() && !ignore.ends_with('\n') {
            ignore.push_str(eol);
        }
        for entry in missing {
            ignore.push_str(entry);
            ignore.push_str(eol);
        }
    }
    Generated {
        files: vec![
            (".gitignore", "config.gitignore", ignore.into_bytes()),
            ("astro.config.mjs", "config.astro", config.into_bytes()),
            (
                "package.json",
                "config.package-manifest",
                package.into_bytes(),
            ),
            ("tsconfig.json", "config.tsconfig", tsconfig.into_bytes()),
        ],
        removed: removed_names,
    }
}

/// A JSON string literal that escapes control, DEL, C1, separator, and
/// bidirectional formatting characters.
pub(crate) fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for scalar in text.chars() {
        match scalar {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            scalar if needs_escape(scalar) => {
                let _ = write!(out, "\\u{:04x}", scalar as u32);
            }
            scalar => out.push(scalar),
        }
    }
    out.push('"');
    out
}

fn needs_escape(scalar: char) -> bool {
    scalar.is_control()
        || matches!(
            scalar,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

// A Markdown code span for source-derived text. Control and formatting
// characters are spelled as escapes, and the fence is longer than any
// backtick run inside.
fn code(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for scalar in text.chars() {
        if needs_escape(scalar) {
            let _ = write!(clean, "\\u{{{:x}}}", scalar as u32);
        } else {
            clean.push(scalar);
        }
    }
    let longest = clean.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if clean.starts_with('`') || clean.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{clean}{pad}{fence}")
}

pub(crate) fn summary(
    version: Option<&str>,
    operations: &[Operation],
    tasks: &[ManualTask],
    line_ending: LineEnding,
) -> String {
    let count = |kind: OperationKind| operations.iter().filter(|o| o.kind == kind).count();
    let mut lines = vec![
        "# Frameshift migration summary".to_owned(),
        String::new(),
        format!(
            "Source: Next.js {} (App Router). Target: Astro with Preact, profile `astro7-preact10`. Rules: `{RULESET}`.",
            version.map_or_else(|| "of unknown version".to_owned(), code)
        ),
        String::new(),
        "Migrate v0 converts only static pages, layouts, and components that the analysis rates `auto` or `review`. Preserved files are copied unchanged below `.frameshift/unmigrated/` and need manual migration. No percentage here claims behavioral equivalence; build and check the result.".to_owned(),
        String::new(),
        "## Results".to_owned(),
        String::new(),
        format!("- Converted to Astro components: {}", count(OperationKind::Create)),
        format!("- Kept as JavaScript or TypeScript with rewritten imports: {}", count(OperationKind::Transform)),
        format!("- Copied unchanged: {}", count(OperationKind::Copy)),
        format!("- Generated project files: {}", count(OperationKind::Config) + 1),
        format!("- Preserved for manual migration: {}", count(OperationKind::Preserve)),
        format!(
            "- Not copied: {}",
            tasks.iter().filter(|t| t.kind == TaskKind::NotCopied).count()
        ),
    ];
    let sections = [
        (TaskKind::Project, "Project tasks"),
        (TaskKind::Preserved, "Preserved files"),
        (TaskKind::Review, "Converted files to review"),
        (TaskKind::NotCopied, "Not copied"),
    ];
    for (kind, title) in sections {
        let selected: Vec<_> = tasks.iter().filter(|t| t.kind == kind).collect();
        if selected.is_empty() {
            continue;
        }
        lines.push(String::new());
        lines.push(format!("## {title}"));
        lines.push(String::new());
        for task in selected {
            match &task.source {
                Some(source) => lines.push(format!("- {}: {}", code(source.as_str()), task.text)),
                None => lines.push(format!("- {}", task.text)),
            }
        }
    }
    if tasks.iter().any(|t| t.kind == TaskKind::Preserved) {
        lines.push(String::new());
        lines.push(format!(
            "Preserved files keep their original package-relative paths below `{UNMIGRATED}/`."
        ));
    }
    lines.push(String::new());
    let eol = match line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
    };
    lines.join(eol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(
            json_string("a\"b\\\u{7f}\u{202e}"),
            "\"a\\\"b\\\\\\u007f\\u202e\""
        );
        assert_eq!(code("app/page.tsx"), "`app/page.tsx`");
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("`x"), "`` `x ``");
        assert_eq!(code("a\u{1b}b"), "`a\\u{1b}b`");
    }
}
