//! The `packageManager` field and package manager detection (`D-069`).

use std::fmt;

use crate::version::{Version, parse_full};

/// A package manager that Frameshift recognizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageManagerKind {
    /// npm
    Npm,
    /// pnpm
    Pnpm,
    /// Yarn, classic or Berry
    Yarn,
    /// Bun
    Bun,
}

impl PackageManagerKind {
    /// Returns the name as `packageManager` spells it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }

    /// Returns this manager's lockfiles in precedence order.
    pub const fn lockfiles(self) -> &'static [LockfileKind] {
        match self {
            Self::Npm => &[LockfileKind::NpmShrinkwrap, LockfileKind::NpmPackageLock],
            Self::Pnpm => &[LockfileKind::Pnpm],
            Self::Yarn => &[LockfileKind::Yarn],
            Self::Bun => &[LockfileKind::BunText, LockfileKind::BunBinary],
        }
    }
}

/// A lockfile that Frameshift recognizes by its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LockfileKind {
    /// `npm-shrinkwrap.json`, which npm prefers over `package-lock.json`.
    NpmShrinkwrap,
    /// `package-lock.json`
    NpmPackageLock,
    /// `pnpm-lock.yaml`
    Pnpm,
    /// `yarn.lock`, classic or Berry
    Yarn,
    /// `bun.lock`, Bun's text lockfile
    BunText,
    /// `bun.lockb`, Bun's binary lockfile, which is never parsed
    BunBinary,
}

impl LockfileKind {
    /// Every kind, in file name order.
    pub const ALL: [Self; 6] = [
        Self::BunText,
        Self::BunBinary,
        Self::NpmShrinkwrap,
        Self::NpmPackageLock,
        Self::Pnpm,
        Self::Yarn,
    ];

    /// Returns the file name.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::NpmShrinkwrap => "npm-shrinkwrap.json",
            Self::NpmPackageLock => "package-lock.json",
            Self::Pnpm => "pnpm-lock.yaml",
            Self::Yarn => "yarn.lock",
            Self::BunText => "bun.lock",
            Self::BunBinary => "bun.lockb",
        }
    }

    /// Returns the package manager that writes this lockfile.
    pub const fn manager(self) -> PackageManagerKind {
        match self {
            Self::NpmShrinkwrap | Self::NpmPackageLock => PackageManagerKind::Npm,
            Self::Pnpm => PackageManagerKind::Pnpm,
            Self::Yarn => PackageManagerKind::Yarn,
            Self::BunText | Self::BunBinary => PackageManagerKind::Bun,
        }
    }
}

/// A valid `packageManager` value: `<name>@<version>[+<algorithm>.<hex>]`,
/// as Corepack defines it, with an exact SemVer version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageManagerSpec {
    /// The package manager.
    pub kind: PackageManagerKind,
    /// The exact version, without build metadata.
    pub version: Version,
    /// The integrity hash Corepack checks: the algorithm and lowercase
    /// hexadecimal digest.
    pub hash: Option<(Box<str>, Box<str>)>,
}

/// Why a `packageManager` value was rejected. Messages never echo the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageManagerError {
    /// The name is not `npm`, `pnpm`, `yarn`, or `bun`, or there is no `@`.
    UnknownManager,
    /// The version is not an exact SemVer version, for example a range, a
    /// tag, a URL, or a version with a `v` prefix.
    InvalidVersion,
    /// The text after `+` is not `<algorithm>.<hex>`.
    InvalidHash,
}

impl fmt::Display for PackageManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownManager => "packageManager does not name npm, pnpm, yarn, or bun",
            Self::InvalidVersion => "packageManager version is not an exact SemVer version",
            Self::InvalidHash => "packageManager hash is not <algorithm>.<hex>",
        })
    }
}

impl std::error::Error for PackageManagerError {}

impl PackageManagerSpec {
    /// Parses a `packageManager` value strictly: no surrounding whitespace,
    /// a lowercase manager name, an exact version, and an optional hash of
    /// a lowercase alphanumeric algorithm name and lowercase hexadecimal digits.
    pub fn parse(text: &str) -> Result<Self, PackageManagerError> {
        let (name, rest) = text
            .split_once('@')
            .ok_or(PackageManagerError::UnknownManager)?;
        let kind = [
            PackageManagerKind::Npm,
            PackageManagerKind::Pnpm,
            PackageManagerKind::Yarn,
            PackageManagerKind::Bun,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
        .ok_or(PackageManagerError::UnknownManager)?;
        let (version_text, hash_text) = match rest.split_once('+') {
            Some((version, hash)) => (version, Some(hash)),
            None => (rest, None),
        };
        let full = parse_full(version_text).ok_or(PackageManagerError::InvalidVersion)?;
        if full.end != version_text.len() || full.build.is_some() {
            return Err(PackageManagerError::InvalidVersion);
        }
        let version =
            Version::parse(version_text).map_err(|_| PackageManagerError::InvalidVersion)?;
        let hash = match hash_text {
            None => None,
            Some(hash) => {
                let (algorithm, digest) = hash
                    .split_once('.')
                    .ok_or(PackageManagerError::InvalidHash)?;
                let algorithm_valid = !algorithm.is_empty()
                    && algorithm
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
                let digest_valid = !digest.is_empty()
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
                if !algorithm_valid || !digest_valid {
                    return Err(PackageManagerError::InvalidHash);
                }
                Some((algorithm.into(), digest.into()))
            }
        };
        Ok(Self {
            kind,
            version,
            hash,
        })
    }
}

/// Where the detected package manager comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionSource {
    /// A valid `packageManager` field.
    Field,
    /// The lockfiles present, which all belong to one package manager.
    Lockfile,
}

impl DetectionSource {
    /// Returns the stable lowercase spelling used in reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Field => "package-manager-field",
            Self::Lockfile => "lockfile",
        }
    }
}

/// Why a present lockfile is not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoredReason {
    /// It belongs to another package manager than the `packageManager` field names.
    OtherManager,
    /// A lockfile of the same package manager takes precedence.
    Superseded,
    /// Lockfiles of several package managers are present and no valid
    /// `packageManager` field decides.
    Ambiguous,
}

impl IgnoredReason {
    /// Returns the stable lowercase spelling used in reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OtherManager => "other-manager",
            Self::Superseded => "superseded",
            Self::Ambiguous => "ambiguous",
        }
    }
}

/// A present lockfile that is not used, with the reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IgnoredLockfile {
    /// The lockfile.
    pub kind: LockfileKind,
    /// Why it is not used.
    pub reason: IgnoredReason,
}

/// The package manager of one package directory and its lockfile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    /// The package manager, or `None` when nothing or something ambiguous
    /// identifies one.
    pub manager: Option<PackageManagerKind>,
    /// Where `manager` comes from.
    pub source: Option<DetectionSource>,
    /// The lockfile to read, the first present one of the manager's
    /// lockfiles in precedence order.
    pub lockfile: Option<LockfileKind>,
    /// Present lockfiles that are not used, in [`LockfileKind::ALL`] order.
    pub ignored: Vec<IgnoredLockfile>,
}

/// Detects the package manager from a valid `packageManager` field and the
/// lockfiles present in the same directory.
///
/// A valid field decides, and lockfiles of other managers are ignored. Without
/// one, lockfiles decide when they all belong to one manager; lockfiles of
/// several managers leave the manager undetermined, and none is used.
pub fn detect(field: Option<&PackageManagerSpec>, present: &[LockfileKind]) -> Detection {
    let is_present = |kind: LockfileKind| present.contains(&kind);
    let (manager, source) = match field {
        Some(spec) => (Some(spec.kind), Some(DetectionSource::Field)),
        None => {
            let mut managers = LockfileKind::ALL
                .into_iter()
                .filter(|kind| is_present(*kind))
                .map(LockfileKind::manager);
            let first = managers.next();
            if first.is_some() && managers.all(|manager| Some(manager) == first) {
                (first, Some(DetectionSource::Lockfile))
            } else {
                (None, None)
            }
        }
    };
    let lockfile = manager.and_then(|manager| {
        manager
            .lockfiles()
            .iter()
            .copied()
            .find(|kind| is_present(*kind))
    });
    let ignored = LockfileKind::ALL
        .into_iter()
        .filter(|kind| is_present(*kind) && Some(*kind) != lockfile)
        .map(|kind| IgnoredLockfile {
            kind,
            reason: match manager {
                None => IgnoredReason::Ambiguous,
                Some(manager) if manager == kind.manager() => IgnoredReason::Superseded,
                Some(_) => IgnoredReason::OtherManager,
            },
        })
        .collect();
    Detection {
        manager,
        source,
        lockfile,
        ignored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_corepack_specifications() {
        let spec = PackageManagerSpec::parse("pnpm@10.24.0+sha512.01ff8ae7").unwrap();
        assert_eq!(spec.kind, PackageManagerKind::Pnpm);
        assert_eq!(spec.version.to_string(), "10.24.0");
        assert_eq!(spec.hash, Some(("sha512".into(), "01ff8ae7".into())));
        assert_eq!(
            PackageManagerSpec::parse("yarn@4.6.0-rc.1")
                .unwrap()
                .version
                .to_string(),
            "4.6.0-rc.1"
        );
        for (text, error) in [
            ("pnpm", PackageManagerError::UnknownManager),
            ("deno@2.0.0", PackageManagerError::UnknownManager),
            ("PNPM@10.0.0", PackageManagerError::UnknownManager),
            (" npm@10.0.0", PackageManagerError::UnknownManager),
            ("npm@^10.0.0", PackageManagerError::InvalidVersion),
            ("npm@latest", PackageManagerError::InvalidVersion),
            ("npm@v10.0.0", PackageManagerError::InvalidVersion),
            ("npm@10.0", PackageManagerError::InvalidVersion),
            ("npm@10.0.0 ", PackageManagerError::InvalidVersion),
            (
                "npm@https://example.test/npm.tgz",
                PackageManagerError::InvalidVersion,
            ),
            ("npm@10.0.0+", PackageManagerError::InvalidHash),
            ("npm@10.0.0+sha512", PackageManagerError::InvalidHash),
            ("npm@10.0.0+sha512.XYZ", PackageManagerError::InvalidHash),
            ("npm@10.0.0+SHA512.abc", PackageManagerError::InvalidHash),
            (
                "npm@10.0.0+sha512.abc.def",
                PackageManagerError::InvalidHash,
            ),
            (
                "npm@9007199254740992.0.0",
                PackageManagerError::InvalidVersion,
            ),
        ] {
            assert_eq!(PackageManagerSpec::parse(text), Err(error), "{text:?}");
        }
    }

    #[test]
    fn detects_from_field_and_lockfiles() {
        use LockfileKind::{BunBinary, BunText, NpmPackageLock, NpmShrinkwrap, Pnpm, Yarn};
        let pnpm = PackageManagerSpec::parse("pnpm@10.0.0").unwrap();
        let detection = detect(Some(&pnpm), &[NpmPackageLock, Pnpm]);
        assert_eq!(detection.manager, Some(PackageManagerKind::Pnpm));
        assert_eq!(detection.source, Some(DetectionSource::Field));
        assert_eq!(detection.lockfile, Some(Pnpm));
        assert_eq!(
            detection.ignored,
            [IgnoredLockfile {
                kind: NpmPackageLock,
                reason: IgnoredReason::OtherManager
            }]
        );
        // A field without its lockfile still decides.
        let detection = detect(Some(&pnpm), &[Yarn]);
        assert_eq!(detection.lockfile, None);
        assert_eq!(detection.ignored[0].reason, IgnoredReason::OtherManager);
        // Lockfiles of one manager decide; precedence within the manager.
        let detection = detect(None, &[NpmPackageLock, NpmShrinkwrap]);
        assert_eq!(detection.manager, Some(PackageManagerKind::Npm));
        assert_eq!(detection.source, Some(DetectionSource::Lockfile));
        assert_eq!(detection.lockfile, Some(NpmShrinkwrap));
        assert_eq!(detection.ignored[0].reason, IgnoredReason::Superseded);
        let detection = detect(None, &[BunBinary, BunText]);
        assert_eq!(detection.lockfile, Some(BunText));
        // Several managers without a field are ambiguous.
        let detection = detect(None, &[Yarn, NpmPackageLock]);
        assert_eq!(detection.manager, None);
        assert_eq!(detection.lockfile, None);
        assert_eq!(
            detection
                .ignored
                .iter()
                .map(|ignored| ignored.kind)
                .collect::<Vec<_>>(),
            [NpmPackageLock, Yarn]
        );
        assert!(
            detection
                .ignored
                .iter()
                .all(|ignored| ignored.reason == IgnoredReason::Ambiguous)
        );
        // Nothing identifies a manager.
        assert_eq!(
            detect(None, &[]),
            Detection {
                manager: None,
                source: None,
                lockfile: None,
                ignored: Vec::new()
            }
        );
    }
}
