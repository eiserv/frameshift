//! Resolves the options that `frameshift.jsonc` can set (`D-103`): flags
//! override the file, and the file overrides the defaults.

use std::io::Read;
use std::path::{Path, PathBuf};

use frameshift_engine::config::{self, PackageChoice, ProjectConfig};
use frameshift_engine::foundation::SourcePath;
use frameshift_engine::limits::Limits;

/// Which configuration a command uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum ConfigChoice {
    /// `frameshift.jsonc` in the source root, if it exists.
    #[default]
    Source,
    /// The file that `--config` names.
    File(PathBuf),
    /// `--no-config`.
    Ignored,
}

/// The configuration-related options of one command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) target: bool,
    pub(crate) package: Option<PackageChoice>,
    pub(crate) config: ConfigChoice,
}

/// Why the options could not be resolved, with its exit status.
pub(crate) struct Failure {
    pub(crate) code: u8,
    pub(crate) message: String,
}

fn read_file(path: &Path, limits: &Limits) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "cannot open the configuration file")?
        .take(limits.text.max_source_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read the configuration file")?;
    Ok(bytes)
}

fn load(source: &Path, choice: &ConfigChoice, limits: &Limits) -> Result<ProjectConfig, String> {
    match choice {
        ConfigChoice::Ignored => Ok(ProjectConfig::default()),
        ConfigChoice::Source => config::load(source, limits)
            .map(Option::unwrap_or_default)
            .map_err(|error| error.to_string()),
        ConfigChoice::File(path) => {
            let bytes = read_file(path, limits)?;
            config::parse(bytes, limits).map_err(|error| error.to_string())
        }
    }
}

impl Selection {
    /// The selected workspace member. `missing_target` is the usage error of
    /// a run in which neither a flag nor the file names the target.
    pub(crate) fn resolve(
        &self,
        source: &Path,
        missing_target: &'static str,
    ) -> Result<Option<SourcePath>, Failure> {
        let limits = Limits::default();
        let file =
            load(source, &self.config, &limits).map_err(|message| Failure { code: 3, message })?;
        if !self.target && !file.target {
            return Err(Failure {
                code: 2,
                message: missing_target.to_owned(),
            });
        }
        Ok(self
            .package
            .clone()
            .or(file.package)
            .and_then(PackageChoice::member))
    }
}
