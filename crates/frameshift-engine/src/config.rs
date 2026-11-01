//! The optional project configuration file `frameshift.jsonc` (`D-027`, `D-103`).
//!
//! The file belongs to the source project, which may be untrusted, so it can
//! only describe the project and the desired migration: its keys cannot enable
//! code execution, change overwrite policies, choose the output location, or
//! name a path outside the source root. Unknown keys are errors.

use std::fmt;
use std::path::Path;

use frameshift_core::{FindingCollector, SourcePath, SourceText};
use frameshift_input::FileRead;
use frameshift_syntax::{JsonDialect, JsonValue, parse_json};

use crate::limits::Limits;

/// The name of the configuration file in the source root.
pub const CONFIG_FILE: &str = "frameshift.jsonc";

const TARGET: &str = "astro-preact";

/// Which package of a workspace a run analyzes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageChoice {
    /// The workspace root.
    Root,
    /// One workspace member, relative to the root.
    Member(SourcePath),
}

impl PackageChoice {
    /// Parses a root-relative directory, with `.` for the root.
    pub fn parse(value: &str, limits: &Limits) -> Option<Self> {
        if value == "." {
            return Some(Self::Root);
        }
        SourcePath::new(value, limits.path).ok().map(Self::Member)
    }

    /// The member path, which is what the analysis options take.
    pub fn member(self) -> Option<SourcePath> {
        match self {
            Self::Root => None,
            Self::Member(path) => Some(path),
        }
    }
}

/// The options that a configuration file sets. Command-line flags override
/// them, and they override the defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    /// Whether the file names the migration target.
    pub target: bool,
    /// The package that the file selects.
    pub package: Option<PackageChoice>,
}

/// Why a configuration cannot be used. The text names no path or value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError(&'static str);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ConfigError {}

/// Parses the bytes of a configuration file.
pub fn parse(bytes: Vec<u8>, limits: &Limits) -> Result<ProjectConfig, ConfigError> {
    let text = SourceText::from_bytes(bytes, limits.text)
        .map_err(|_| ConfigError("the configuration file is not bounded UTF-8 text"))?;
    let document = parse_json(&text, JsonDialect::Jsonc, limits.json)
        .map_err(|_| ConfigError("the configuration file is not valid JSONC"))?;
    if document.duplicate_keys().len() != 0 {
        return Err(ConfigError("the configuration file repeats a key"));
    }
    let Some(JsonValue::Object(root)) = document.value(document.root()) else {
        return Err(ConfigError("the configuration file must hold an object"));
    };
    let mut config = ProjectConfig::default();
    for member in root.members() {
        let value = document.value(member.value());
        match (member.key(), value) {
            ("$schema", Some(JsonValue::String(_))) => {}
            ("to", Some(JsonValue::String(TARGET))) => config.target = true,
            ("to", _) => {
                return Err(ConfigError("the only available target is astro-preact"));
            }
            ("package", Some(JsonValue::String(package))) => {
                config.package = Some(PackageChoice::parse(package, limits).ok_or(ConfigError(
                    "the configured package must be a canonical root-relative directory",
                ))?);
            }
            ("$schema" | "package", _) => {
                return Err(ConfigError("a configuration value must be a string"));
            }
            _ => return Err(ConfigError("the configuration file has an unknown key")),
        }
    }
    Ok(config)
}

/// Reads `frameshift.jsonc` from the source root through the snapshot store,
/// so its links and ignore rules behave like every other source file. A file
/// that exists but is not admitted, such as a link or an ignored file, is an
/// error rather than an absent configuration.
pub fn load(root: &Path, limits: &Limits) -> Result<Option<ProjectConfig>, ConfigError> {
    let unreadable = ConfigError("the source or its configuration file cannot be read");
    let path = SourcePath::new(CONFIG_FILE, limits.path)
        .map_err(|_| ConfigError("the configuration file name is invalid"))?;
    // The analysis reports discovery findings; this read needs only the file.
    let mut findings = FindingCollector::new(limits.finding);
    let snapshot = frameshift_input::snapshot(root, limits.snapshot(), &mut findings)
        .map_err(|_| unreadable)?;
    let inventory = snapshot.inventory();
    let Some(id) = inventory.files().find(&path) else {
        return if inventory.entry(&path).is_some() {
            Err(ConfigError(
                "the configuration file is not an admitted regular file; use --config or --no-config",
            ))
        } else {
            Ok(None)
        };
    };
    match snapshot.read(id) {
        Ok(FileRead::Content(file)) => parse(file.bytes().to_vec(), limits).map(Some),
        Ok(FileRead::Unreadable(_)) | Err(_) => Err(unreadable),
    }
}
