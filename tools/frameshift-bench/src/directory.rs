//! Read-only measurement of source files in a directory (`D-043`).
//!
//! The E1 sample of real-world files is kept outside the repository. This
//! module walks such a directory without following symbolic links, selects
//! JavaScript and TypeScript files by extension, maps each extension to a
//! dialect and source goal, and parses every file. It never writes, executes,
//! or interprets project configuration. The extension mapping is a property
//! of this measurement tool, not the product's file classification, which
//! belongs to the planned project layer (M2, M4).

use std::{
    fs,
    path::{Path, PathBuf},
};

use frameshift_engine::{
    JsDialect, LexicalErrorKind, LexicalLimits, ParseError, ParseErrorKind, ParseLimits, Position,
    SourceGoal, SourceText, TextError, TextLimits, parse_js_program,
};

use crate::corpus::CorpusFile;

/// Directories that never hold project sources: version control data and
/// installed packages. A fresh checkout contains no build output.
const IGNORED_DIRECTORIES: [&str; 2] = [".git", "node_modules"];

/// Directory levels below the root that the walk descends. Deeper entries
/// are counted as skipped rather than read.
const MAX_DEPTH: usize = 64;

/// File name extensions that select a file, in report order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Extension {
    /// `.js`
    Js,
    /// `.jsx`
    Jsx,
    /// `.mjs`
    Mjs,
    /// `.cjs`
    Cjs,
    /// `.ts`
    Ts,
    /// `.tsx`
    Tsx,
    /// `.mts`
    Mts,
    /// `.cts`
    Cts,
    /// Declaration files: `.d.ts`, `.d.mts`, `.d.cts`, and other `.ts` files
    /// whose name contains `.d.`, such as `styles.d.css.ts`
    Declaration,
}

impl Extension {
    /// Every extension, in report order.
    pub const ALL: [Self; 9] = [
        Self::Js,
        Self::Jsx,
        Self::Mjs,
        Self::Cjs,
        Self::Ts,
        Self::Tsx,
        Self::Mts,
        Self::Cts,
        Self::Declaration,
    ];

    /// Selects a file by the last extension of its name, case-sensitively,
    /// as Node.js and the TypeScript compiler do. Declaration files follow
    /// the compiler's `getDeclarationFileExtension`.
    pub fn of(name: &str) -> Option<Self> {
        let (stem, extension) = name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        if name.ends_with(".d.mts")
            || name.ends_with(".d.cts")
            || (extension == "ts" && name.contains(".d."))
        {
            return Some(Self::Declaration);
        }
        match extension {
            "js" => Some(Self::Js),
            "jsx" => Some(Self::Jsx),
            "mjs" => Some(Self::Mjs),
            "cjs" => Some(Self::Cjs),
            "ts" => Some(Self::Ts),
            "tsx" => Some(Self::Tsx),
            "mts" => Some(Self::Mts),
            "cts" => Some(Self::Cts),
            _ => None,
        }
    }

    /// The extension with its leading dot.
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Js => ".js",
            Self::Jsx => ".jsx",
            Self::Mjs => ".mjs",
            Self::Cjs => ".cjs",
            Self::Ts => ".ts",
            Self::Tsx => ".tsx",
            Self::Mts => ".mts",
            Self::Cts => ".cts",
            Self::Declaration => ".d.ts",
        }
    }

    /// The dialect and source goal this tool parses the file with.
    ///
    /// Next.js compiles JSX in `.js` files, so `.js` and `.jsx` use the JSX
    /// dialect. Node.js loads `.mjs` and `.cjs` directly, without JSX, as a
    /// module and a CommonJS script. TypeScript sources use module syntax
    /// whatever their output format, so every TypeScript extension uses the
    /// Module goal. A `.js` or `.jsx` file that fails as a module is parsed
    /// again as a script ([`Outcome::ScriptOnly`]), because its goal depends
    /// on configuration that this tool does not read.
    pub fn mapping(self) -> (JsDialect, SourceGoal) {
        match self {
            Self::Js | Self::Jsx => (JsDialect::Jsx, SourceGoal::Module),
            Self::Mjs => (JsDialect::JavaScript, SourceGoal::Module),
            Self::Cjs => (JsDialect::JavaScript, SourceGoal::Script),
            Self::Ts | Self::Mts | Self::Cts => (JsDialect::TypeScript, SourceGoal::Module),
            Self::Tsx => (JsDialect::Tsx, SourceGoal::Module),
            Self::Declaration => (JsDialect::TypeScriptDeclaration, SourceGoal::Module),
        }
    }
}

/// One selected file.
pub struct SampleFile {
    /// Path relative to the root, with `/` separators.
    pub path: String,
    /// The extension that selected the file.
    pub extension: Extension,
    /// Size of the file in bytes.
    pub bytes: usize,
    /// How the file parsed.
    pub outcome: Outcome,
}

/// A parse error with its derived one-based position.
pub struct Failure {
    /// The error the parser returned.
    pub error: ParseError,
    /// Line and column of the error offset.
    pub position: Position,
}

/// The result of parsing one file.
pub enum Outcome {
    /// Parsed in its mapped dialect and goal with the default limits.
    Accepted(CorpusFile),
    /// Parsed only after the node, token, and lexical limits were raised;
    /// the default limits stopped it with the recorded failure.
    BeyondLimits(Failure),
    /// A `.js` or `.jsx` file that failed as a module with the recorded
    /// failure and parsed as a script.
    ScriptOnly(Failure),
    /// Rejected in its mapped dialect and goal, with raised limits where the
    /// default limits stopped it first.
    Rejected(Failure),
    /// The bytes are not acceptable source text, for example not UTF-8.
    Text(TextError),
    /// The file could not be read.
    Unreadable,
}

/// Entries that the walk did not select or could not visit.
#[derive(Default)]
pub struct Skipped {
    /// Symbolic links, which are never followed.
    pub links: usize,
    /// Directories named in [`IGNORED_DIRECTORIES`].
    pub ignored_directories: usize,
    /// Directories that could not be listed, or lie deeper than [`MAX_DEPTH`].
    pub unvisited_directories: usize,
    /// Files without a selected extension.
    pub other_files: usize,
}

/// The selected files of a directory, sorted by path.
pub struct Sample {
    /// The files, sorted byte-wise by relative path.
    pub files: Vec<SampleFile>,
    /// What the walk left out.
    pub skipped: Skipped,
}

impl Sample {
    /// Walks `root` and parses every selected file.
    pub fn read(root: &Path) -> Result<Self, String> {
        let metadata = fs::metadata(root)
            .map_err(|error| format!("cannot read {}: {error}", root.display()))?;
        if !metadata.is_dir() {
            return Err(format!("{} is not a directory", root.display()));
        }
        let (paths, skipped) = walk(root);
        let files = paths
            .into_iter()
            .map(|(path, relative, extension)| {
                let (bytes, outcome) = match fs::read(&path) {
                    Ok(bytes) => (bytes.len(), parse(bytes, extension)),
                    Err(_) => (0, Outcome::Unreadable),
                };
                SampleFile {
                    path: relative,
                    extension,
                    bytes,
                    outcome,
                }
            })
            .collect();
        Ok(Self { files, skipped })
    }
}

// Collects the selected files below `root`, sorted by relative path.
fn walk(root: &Path) -> (Vec<(PathBuf, String, Extension)>, Skipped) {
    let mut skipped = Skipped::default();
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((directory, prefix, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            skipped.unvisited_directories += 1;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                skipped.unvisited_directories += 1;
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            // The entry's own type: a link is never resolved.
            let Ok(kind) = entry.file_type() else {
                skipped.unvisited_directories += 1;
                continue;
            };
            if kind.is_symlink() {
                skipped.links += 1;
            } else if kind.is_dir() {
                if IGNORED_DIRECTORIES.contains(&name.as_str()) {
                    skipped.ignored_directories += 1;
                } else if depth >= MAX_DEPTH {
                    skipped.unvisited_directories += 1;
                } else {
                    pending.push((entry.path(), relative, depth + 1));
                }
            } else {
                match Extension::of(&name) {
                    Some(extension) => files.push((entry.path(), relative, extension)),
                    None => skipped.other_files += 1,
                }
            }
        }
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));
    (files, skipped)
}

/// Parses one file's bytes as [`Extension::mapping`] selects.
pub fn parse(bytes: Vec<u8>, extension: Extension) -> Outcome {
    let source = match SourceText::from_bytes(bytes, TextLimits::default()) {
        Ok(source) => source,
        Err(error) => return Outcome::Text(error),
    };
    let (dialect, goal) = extension.mapping();
    let error = match parse_js_program(&source, goal, dialect, ParseLimits::default()) {
        Ok(_) => {
            return Outcome::Accepted(CorpusFile {
                goal,
                dialect,
                text: source.as_str().to_owned(),
            });
        }
        Err(error) => error,
    };
    let failure = |error: ParseError| Failure {
        // Parse errors point into the source, so the lookup cannot fail; a
        // zero position would expose a broken invariant in the report.
        position: source.position(error.offset).unwrap_or(Position {
            offset: error.offset,
            line: 0,
            column: 0,
        }),
        error,
    };
    if is_limit(&error) {
        return match parse_js_program(&source, goal, dialect, raised_limits()) {
            Ok(_) => Outcome::BeyondLimits(failure(error)),
            Err(error) => Outcome::Rejected(failure(error)),
        };
    }
    if matches!(extension, Extension::Js | Extension::Jsx)
        && parse_js_program(&source, SourceGoal::Script, dialect, raised_limits()).is_ok()
    {
        return Outcome::ScriptOnly(failure(error));
    }
    Outcome::Rejected(failure(error))
}

// Whether a configured bound, rather than the grammar, stopped the parser.
fn is_limit(error: &ParseError) -> bool {
    matches!(
        error.kind,
        ParseErrorKind::ResourceLimit
            | ParseErrorKind::Lexical(
                LexicalErrorKind::CommentLimit
                    | LexicalErrorKind::StringLimit
                    | LexicalErrorKind::NumberLimit
                    | LexicalErrorKind::TemplateLimit
                    | LexicalErrorKind::IdentifierLimit
                    | LexicalErrorKind::RegExpLimit
                    | LexicalErrorKind::JsxTextLimit
            )
    )
}

// Node, token, and lexical bounds no source text within the default text
// limit can reach. The nesting bound stays, because it protects the stack.
fn raised_limits() -> ParseLimits {
    let bytes = TextLimits::default().max_source_bytes;
    ParseLimits {
        lexical: LexicalLimits {
            max_comments: usize::MAX,
            max_string_bytes: bytes,
            max_number_bytes: bytes,
            max_template_bytes: bytes,
            max_identifier_bytes: bytes,
            max_regexp_bytes: bytes,
            max_jsx_text_bytes: bytes,
        },
        max_nodes: usize::MAX,
        max_tokens: usize::MAX,
        ..ParseLimits::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_select_files_by_their_last_extension() {
        assert_eq!(Extension::of("page.tsx"), Some(Extension::Tsx));
        assert_eq!(Extension::of("next-env.d.ts"), Some(Extension::Declaration));
        assert_eq!(Extension::of("index.d.mts"), Some(Extension::Declaration));
        assert_eq!(
            Extension::of("styles.d.css.ts"),
            Some(Extension::Declaration)
        );
        assert_eq!(Extension::of("id.ts"), Some(Extension::Ts));
        assert_eq!(Extension::of("index.d.tsx"), Some(Extension::Tsx));
        assert_eq!(Extension::of("next.config.mjs"), Some(Extension::Mjs));
        assert_eq!(Extension::of("postcss.config.cjs"), Some(Extension::Cjs));
        assert_eq!(Extension::of("route.mts"), Some(Extension::Mts));
        for name in [
            "styles.css",
            "page.TSX",
            "README",
            ".js",
            "data.json",
            "a.js.map",
        ] {
            assert_eq!(Extension::of(name), None, "{name}");
        }
        let spellings: Vec<&str> = Extension::ALL.iter().map(|e| e.spelling()).collect();
        assert_eq!(
            spellings,
            [
                ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".d.ts"
            ]
        );
    }

    #[test]
    fn outcomes_follow_the_extension_mapping() {
        let accepted = |text: &str, extension| {
            matches!(
                parse(text.as_bytes().to_vec(), extension),
                Outcome::Accepted(_)
            )
        };
        assert!(accepted("export const a = <b />;", Extension::Js));
        assert!(accepted("let a = <T,>(x: T) => x;", Extension::Tsx));
        assert!(accepted("let a = <T>x;", Extension::Ts));
        assert!(accepted("module.exports = {};", Extension::Cjs));
        assert!(accepted("export const a: number;", Extension::Declaration));
        assert!(!accepted("export const a: number;", Extension::Ts));
        assert!(!accepted("let a = <b />;", Extension::Mjs));
        assert!(!accepted("let a = <T>x;", Extension::Tsx));
        assert!(matches!(
            parse(b"with (a) {}".to_vec(), Extension::Js),
            Outcome::ScriptOnly(_)
        ));
        assert!(matches!(
            parse(b"with (a) {}".to_vec(), Extension::Mjs),
            Outcome::Rejected(_)
        ));
        assert!(matches!(
            parse(vec![0xFF], Extension::Js),
            Outcome::Text(TextError::InvalidUtf8 { .. })
        ));
        let many = "a;".repeat(ParseLimits::default().max_nodes);
        assert!(matches!(
            parse(many.into_bytes(), Extension::Ts),
            Outcome::BeyondLimits(_)
        ));
    }

    #[test]
    fn the_walk_sorts_files_and_skips_ignored_directories() {
        let root =
            std::env::temp_dir().join(format!("frameshift-bench-walk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for directory in ["app/blog", "node_modules/pkg", ".git", "lib"] {
            fs::create_dir_all(root.join(directory)).expect("create test directory");
        }
        for (file, text) in [
            (
                "app/page.tsx",
                "export default function P() { return <main />; }",
            ),
            ("app/blog/page.js", "export default () => <p>hi</p>;"),
            ("lib/a.ts", "export const a: number = 1;"),
            ("lib/broken.ts", "export const = 1;"),
            ("node_modules/pkg/index.js", "module.exports = 1;"),
            (".git/hook.js", "x"),
            ("styles.css", "a {}"),
        ] {
            fs::write(root.join(file), text).expect("write test file");
        }
        let sample = Sample::read(&root);
        let _ = fs::remove_dir_all(&root);
        let sample = sample.expect("readable directory");
        let paths: Vec<&str> = sample.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "app/blog/page.js",
                "app/page.tsx",
                "lib/a.ts",
                "lib/broken.ts"
            ]
        );
        assert!(matches!(sample.files[3].outcome, Outcome::Rejected(_)));
        assert_eq!(sample.skipped.ignored_directories, 2);
        assert_eq!(sample.skipped.other_files, 1);
    }
}
