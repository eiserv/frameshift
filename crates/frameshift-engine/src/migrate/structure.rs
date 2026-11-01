//! The structural postcondition of every generated script (`D-107`).
//!
//! A file that fails these checks is preserved instead of written, so a rule
//! defect cannot produce output that no longer parses, imports a file that
//! the plan does not write, or still depends on the source framework. The
//! template of an Astro file is outside this check: it needs a parser that
//! does not exist yet.

use std::collections::BTreeSet;

use frameshift_core::SourceText;
use frameshift_syntax::{
    JsDialect, NodeId, SourceGoal, SyntaxNodeKind, SyntaxTree, TypeScriptKind, parse_js_program,
};

use crate::limits::Limits;

// Extensions that an extensionless relative specifier can resolve to.
const SCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "js", "jsx", "mjs", "d.ts"];

// The frontmatter of a generated Astro file, without its fences. A file
// without an opening fence is all template, and an unclosed one is an error.
fn frontmatter(text: &str) -> Result<Option<&str>, String> {
    let Some(rest) = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))
    else {
        return Ok(None);
    };
    rest.match_indices("---")
        .find(|(index, _)| *index == 0 || rest.as_bytes()[index - 1] == b'\n')
        .map(|(end, _)| Some(&rest[..end]))
        .ok_or_else(|| "the generated Astro file has no closed frontmatter".to_owned())
}

// The dialect and source text of a generated file, if it is a script.
fn script<'t>(output: &str, text: &'t str) -> Result<Option<(JsDialect, &'t str)>, String> {
    let dialect = if output.ends_with(".astro") {
        return Ok(frontmatter(text)?.map(|code| (JsDialect::TypeScript, code)));
    } else if output.ends_with(".d.ts") {
        JsDialect::TypeScriptDeclaration
    } else if output.ends_with(".tsx") {
        JsDialect::Tsx
    } else if output.ends_with(".ts") {
        JsDialect::TypeScript
    } else if output.ends_with(".jsx") || output.ends_with(".js") || output.ends_with(".mjs") {
        JsDialect::Jsx
    } else {
        return Ok(None);
    };
    Ok(Some((dialect, text)))
}

/// Checks one generated file against the set of output paths that the plan
/// writes. Files that are not scripts or Astro components have no structure
/// to check here.
pub(super) fn check(
    output: &str,
    bytes: &[u8],
    outputs: &BTreeSet<String>,
    limits: &Limits,
) -> Result<(), String> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        // Assets may hold any bytes; a generated script never does.
        return if script(output, "---\n---\n")?.is_some() {
            Err("the generated file is not UTF-8".to_owned())
        } else {
            Ok(())
        };
    };
    let Some((dialect, code)) = script(output, text)? else {
        return Ok(());
    };
    let source = SourceText::new(code.to_owned(), limits.text)
        .map_err(|_| "the generated file exceeds the text limit".to_owned())?;
    let tree = parse_js_program(&source, SourceGoal::Module, dialect, limits.parse)
        .map_err(|_| "the generated file does not parse again".to_owned())?;
    // Declaration files only describe types and keep the source's references.
    if dialect == JsDialect::TypeScriptDeclaration {
        return Ok(());
    }
    visit(&tree, tree.root(), output, outputs)
}

fn visit(
    tree: &SyntaxTree<'_>,
    id: NodeId,
    output: &str,
    outputs: &BTreeSet<String>,
) -> Result<(), String> {
    let Some(node) = tree.node(id) else {
        return Err("a syntax node is outside the tree".to_owned());
    };
    let kind = node.kind();
    // Types do not exist at run time, so they can neither break nor keep a dependency.
    if matches!(
        kind,
        SyntaxNodeKind::TypeScript {
            kind: TypeScriptKind::TypeOnly,
            ..
        }
    ) {
        return Ok(());
    }
    if matches!(kind, SyntaxNodeKind::Directive { .. }) {
        let directive = tree
            .source()
            .slice(node.span())
            .map_err(|_| "a syntax span is outside the source".to_owned())?;
        if directive.contains("use client") || directive.contains("use server") {
            return Err("the generated file keeps a React directive".to_owned());
        }
    }
    let source = if let SyntaxNodeKind::ImportDeclaration { source, .. }
    | SyntaxNodeKind::ExportAll { source, .. }
    | SyntaxNodeKind::ImportCall { source, .. } = kind
    {
        Some(source)
    } else if let SyntaxNodeKind::ExportNamed { source, .. } = kind {
        source
    } else {
        None
    };
    if let Some(specifier) = source.and_then(|source| tree.string_literal_value(source)) {
        check_specifier(&specifier, output, outputs)?;
    }
    for child in tree.child_nodes(id).into_iter().flatten() {
        visit(tree, child, output, outputs)?;
    }
    Ok(())
}

fn check_specifier(
    specifier: &str,
    output: &str,
    outputs: &BTreeSet<String>,
) -> Result<(), String> {
    if specifier == "next" || specifier.starts_with("next/") {
        return Err("the generated file still imports Next.js".to_owned());
    }
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return Ok(());
    }
    let Some(target) = join(output, specifier) else {
        return Err("a generated import leaves the output".to_owned());
    };
    let resolves = outputs.contains(&target)
        || SCRIPT_EXTENSIONS.iter().any(|extension| {
            outputs.contains(&format!("{target}.{extension}"))
                || outputs.contains(&format!("{target}/index.{extension}"))
        });
    if resolves {
        Ok(())
    } else {
        Err("a generated import names a file that the plan does not write".to_owned())
    }
}

// The output path that a relative specifier names from the file at `output`.
fn join(output: &str, specifier: &str) -> Option<String> {
    let mut parts: Vec<&str> = output.split('/').collect();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(output: &str, text: &str) -> Result<(), String> {
        let outputs: BTreeSet<String> = [
            "src/lib/site.ts",
            "src/lib/index.ts",
            "src/layouts/RootLayout.astro",
            "src/styles/a.css",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        check(output, text.as_bytes(), &outputs, &Limits::default())
    }

    #[test]
    fn accepts_what_the_rules_generate() {
        for (output, text) in [
            (
                "src/pages/index.astro",
                "---\nimport { site } from '../lib/site';\nimport RootLayout from '../layouts/RootLayout.astro';\nimport '../styles/a.css';\nconst a = await Promise.resolve(site);\n---\n<RootLayout><p>{a}</p></RootLayout>\n",
            ),
            (
                "src/layouts/Bare.astro",
                "<html><body><slot /></body></html>\n",
            ),
            (
                "src/pages/x.astro",
                "---\r\nconst a = 1;\r\n---\r\n<p />\r\n",
            ),
            (
                "src/lib/other.ts",
                "export { site } from './site';\nexport * from '.';\n",
            ),
            (
                "src/lib/kept.tsx",
                "import type { Metadata } from 'next';\nexport const a = <p />;\n",
            ),
            (
                "src/types/a.d.ts",
                "import type { X } from 'next';\nimport { Y } from './missing';\n",
            ),
            ("src/styles/b.css", "not a script {"),
            ("public/a.bin", "\u{0}\u{1}"),
        ] {
            assert_eq!(run(output, text), Ok(()), "{output}");
        }
    }

    #[test]
    fn refuses_broken_files_without_echoing_them() {
        for (output, text) in [
            ("src/pages/a.astro", "---\nconst a = 1;\n"),
            (
                "src/pages/a.astro",
                "---\nconst = secret-name;\n---\n<p />\n",
            ),
            ("src/lib/a.ts", "export const = secret-name;\n"),
            (
                "src/lib/a.ts",
                "import x from 'next/link';\nexport { x };\n",
            ),
            ("src/lib/a.ts", "import x from 'next';\nexport { x };\n"),
            ("src/lib/a.ts", "export * from './secret-name';\n"),
            (
                "src/lib/a.ts",
                "export { a } from '../../../../secret-name';\n",
            ),
            ("src/lib/a.ts", "const a = import('./secret-name');\n"),
            (
                "src/pages/a.astro",
                "---\nimport a from '../lib/secret-name';\n---\n<p />\n",
            ),
            ("src/lib/a.tsx", "'use client';\nexport const a = 1;\n"),
        ] {
            let error = run(output, text).expect_err(text);
            assert!(!error.contains("secret-name"), "{text}: {error}");
        }
    }
}
