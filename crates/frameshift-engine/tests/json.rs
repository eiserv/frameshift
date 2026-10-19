//! JSON and JSONC through the public facade, on fictional project files (`D-068`).

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::limits::Limits;
use frameshift_engine::syntax::{
    JsonDialect, JsonDocument, JsonErrorKind, JsonExpected, JsonId, JsonValue, parse_json,
};

fn text(source: &str) -> SourceText {
    SourceText::new(source.to_owned(), TextLimits::default()).unwrap()
}

fn lookup<'a>(document: &'a JsonDocument<'_>, path: &[&str]) -> Option<JsonValue<'a>> {
    let mut id: JsonId = document.root();
    for key in path {
        id = document.value(id)?.as_object()?.get(key)?;
    }
    document.value(id)
}

const PACKAGE: &str = "\u{FEFF}{
  \"name\": \"lumen-site\",
  \"version\": \"0.1.0\",
  \"private\": true,
  \"packageManager\": \"pnpm@9.15.0\",
  \"scripts\": { \"build\": \"next build\" },
  \"dependencies\": {
    \"next\": \"16.2.1\",
    \"react\": \"^19.1.0\",
    \"react-dom\": \"^19.1.0\"
  },
  \"workspaces\": [\"packages/*\"]
}
";

const TSCONFIG: &str = "{
  // Fictional TypeScript configuration with comments and trailing commas.
  \"compilerOptions\": {
    \"strict\": true,
    \"target\": \"ES2022\",
    /* Path aliases */
    \"paths\": { \"@/*\": [\"./src/*\",], },
  },
  \"include\": [\"next-env.d.ts\", \"**/*.ts\", \"**/*.tsx\"],
}
";

const BUN_LOCK: &str = "{
  \"lockfileVersion\": 1,
  \"workspaces\": {
    \"\": { \"name\": \"lumen-site\", \"dependencies\": { \"next\": \"16.2.1\", }, },
  },
  \"packages\": {
    \"next\": [\"next@16.2.1\", \"\", {}, \"sha512-fictional\"],
  },
}
";

#[test]
fn manifests_and_configuration_files_parse_with_their_dialects() {
    let limits = Limits::default().json;
    let package = text(PACKAGE);
    let manifest = parse_json(&package, JsonDialect::Json, limits).unwrap();
    assert!(manifest.has_bom());
    assert_eq!(
        lookup(&manifest, &["dependencies", "next"]).and_then(JsonValue::as_str),
        Some("16.2.1")
    );
    assert_eq!(
        lookup(&manifest, &["packageManager"]).and_then(JsonValue::as_str),
        Some("pnpm@9.15.0")
    );
    assert_eq!(
        lookup(&manifest, &["private"]).and_then(JsonValue::as_bool),
        Some(true)
    );
    let workspaces = lookup(&manifest, &["workspaces"])
        .and_then(JsonValue::as_array)
        .unwrap();
    assert_eq!(workspaces.len(), 1);
    assert_eq!(manifest.duplicate_keys().len(), 0);

    let tsconfig = text(TSCONFIG);
    let error = parse_json(&tsconfig, JsonDialect::Json, limits).unwrap_err();
    assert_eq!(error.kind(), JsonErrorKind::CommentNotAllowed);
    let config = parse_json(&tsconfig, JsonDialect::Jsonc, limits).unwrap();
    assert_eq!(
        lookup(&config, &["compilerOptions", "strict"]).and_then(JsonValue::as_bool),
        Some(true)
    );
    let paths = lookup(&config, &["compilerOptions", "paths", "@/*"])
        .and_then(JsonValue::as_array)
        .unwrap();
    assert_eq!(paths.len(), 1);

    let lock = text(BUN_LOCK);
    let lockfile = parse_json(&lock, JsonDialect::Jsonc, limits).unwrap();
    let version = lookup(&lockfile, &["lockfileVersion"])
        .and_then(JsonValue::as_number)
        .and_then(|number| number.as_u64());
    assert_eq!(version, Some(1));
    assert_eq!(
        lookup(&lockfile, &["workspaces", "", "dependencies", "next"]).and_then(JsonValue::as_str),
        Some("16.2.1")
    );
}

#[test]
fn duplicate_manifest_keys_are_visible_and_resolve_like_node() {
    let source = text("{\"name\": \"first\", \"dependencies\": {}, \"name\": \"second\"}");
    let document = parse_json(&source, JsonDialect::Json, Limits::default().json).unwrap();
    assert_eq!(
        lookup(&document, &["name"]).and_then(JsonValue::as_str),
        Some("second")
    );
    let duplicates: Vec<_> = document
        .duplicate_keys()
        .map(|member| (member.key(), member.key_span().start()))
        .collect();
    assert_eq!(duplicates, [("name", 1)]);
}

#[test]
fn hostile_inputs_fail_with_bounded_errors() {
    let limits = Limits::default().json;
    let deep = "[".repeat(100_000);
    let deep = text(&deep);
    let error = parse_json(&deep, JsonDialect::Json, limits).unwrap_err();
    assert_eq!(error.kind(), JsonErrorKind::DepthLimit { limit: 256 });
    assert_eq!(error.span().start(), 256);

    let unterminated = text(&format!("[\"{}", "x".repeat(1_000_000)));
    let error = parse_json(&unterminated, JsonDialect::Json, limits).unwrap_err();
    assert_eq!(error.kind(), JsonErrorKind::UnterminatedString);

    let comment = text(&format!("/*{}", "*".repeat(100_000)));
    let error = parse_json(&comment, JsonDialect::Jsonc, limits).unwrap_err();
    assert_eq!(error.kind(), JsonErrorKind::UnterminatedComment);

    for source in ["", "{", "{\"a\"", "[1,", "\"\\ud800\""] {
        let source = text(source);
        let error = parse_json(&source, JsonDialect::Jsonc, limits).unwrap_err();
        assert!(error.span().end() <= source.len());
        assert!(source.as_str().is_char_boundary(error.span().start()));
        assert!(!error.to_string().contains("ud800"));
    }
    let end = text("[1");
    assert_eq!(
        parse_json(&end, JsonDialect::Json, limits)
            .unwrap_err()
            .kind(),
        JsonErrorKind::UnexpectedEnd {
            expected: JsonExpected::ArrayContinuation
        }
    );
}
