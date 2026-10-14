//! Canonical path values, independent of host filesystem interpretation.

use std::fmt;

/// Explicit, inclusive limits for canonical UTF-8 paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathLimits {
    /// Maximum path size in UTF-8 bytes, including separators. Defaults to 32 KiB.
    pub max_path_bytes: usize,
    /// Maximum number of slash-separated components. Defaults to 256.
    pub max_components: usize,
}

impl Default for PathLimits {
    fn default() -> Self {
        Self {
            max_path_bytes: 32 * 1024,
            max_components: 256,
        }
    }
}

/// A rejected path value. Errors contain counts and offsets, never input text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    /// The empty string does not identify a file or directory below the root.
    Empty,
    /// The input exceeds the explicit byte limit, checked before decoding.
    TooLong {
        /// Actual input size in bytes.
        bytes: usize,
        /// Maximum permitted size in bytes.
        limit: usize,
    },
    /// The input is not valid UTF-8.
    InvalidUtf8 {
        /// Byte offset before which the input is valid UTF-8.
        valid_up_to: usize,
    },
    /// A leading slash makes the path absolute.
    Absolute,
    /// An initial ASCII letter and colon could be a native drive prefix.
    DrivePrefix,
    /// A component exceeds the explicit count limit.
    TooManyComponents {
        /// Byte offset of the first component beyond the limit.
        offset: usize,
        /// Maximum permitted component count.
        limit: usize,
    },
    /// Leading, repeated, or trailing separators introduce an empty component.
    EmptyComponent {
        /// Byte offset at the start of the empty component, possibly EOF.
        offset: usize,
    },
    /// A component is exactly `.` or `..`.
    DotComponent {
        /// Byte offset of the component.
        offset: usize,
    },
    /// Backslashes are not canonical separators or supported source-name bytes.
    Backslash {
        /// Byte offset of the backslash.
        offset: usize,
    },
    /// NUL cannot be part of a filesystem name.
    Nul {
        /// Byte offset of NUL.
        offset: usize,
    },
    /// A Unicode control character cannot be carried into portable output.
    OutputControl {
        /// Byte offset of the control character.
        offset: usize,
    },
    /// A Windows-reserved punctuation character occurs in an output component.
    OutputCharacter {
        /// Byte offset of the character.
        offset: usize,
    },
    /// An output component ends in an ASCII space or dot.
    OutputEnding {
        /// Byte offset of the final space or dot.
        offset: usize,
    },
    /// An output component names a Windows device, optionally with an extension.
    OutputDevice {
        /// Byte offset of the component.
        offset: usize,
    },
    /// An output component exceeds the conservative 255 UTF-8 byte bound.
    OutputComponentTooLong {
        /// Byte offset of the component.
        offset: usize,
        /// Actual component size in UTF-8 bytes.
        bytes: usize,
    },
    /// Storage could not be reserved for a validated value.
    AllocationFailed,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("path is empty"),
            Self::TooLong { bytes, limit } => {
                write!(f, "path size {bytes} exceeds byte limit {limit}")
            }
            Self::InvalidUtf8 { valid_up_to } => {
                write!(f, "path is not UTF-8 at byte {valid_up_to}")
            }
            Self::Absolute => f.write_str("path must be relative to the project root"),
            Self::DrivePrefix => f.write_str("path contains a native drive prefix"),
            Self::TooManyComponents { offset, limit } => {
                write!(f, "path exceeds component limit {limit} at byte {offset}")
            }
            Self::EmptyComponent { offset } => {
                write!(f, "path has an empty component at byte {offset}")
            }
            Self::DotComponent { offset } => {
                write!(f, "path has a dot component at byte {offset}")
            }
            Self::Backslash { offset } => {
                write!(f, "path has a backslash at byte {offset}")
            }
            Self::Nul { offset } => write!(f, "path has NUL at byte {offset}"),
            Self::OutputControl { offset } => {
                write!(f, "output path has a control character at byte {offset}")
            }
            Self::OutputCharacter { offset } => {
                write!(f, "output path has reserved punctuation at byte {offset}")
            }
            Self::OutputEnding { offset } => {
                write!(
                    f,
                    "output path has a trailing space or dot at byte {offset}"
                )
            }
            Self::OutputDevice { offset } => {
                write!(f, "output path names a reserved device at byte {offset}")
            }
            Self::OutputComponentTooLong { offset, bytes } => {
                write!(
                    f,
                    "output component at byte {offset} has {bytes} bytes, exceeding 255"
                )
            }
            Self::AllocationFailed => f.write_str("path storage allocation failed"),
        }
    }
}

impl std::error::Error for PathError {}

/// A nonempty canonical, root-relative UTF-8 source path, with `/` separators.
///
/// Spelling, case, and Unicode normalization are preserved. Nonportable names,
/// including Windows device names, are retained for analysis. Backslashes,
/// NUL, drive prefixes, empty components, and dot components are rejected rather
/// than normalized. Ordering compares original UTF-8 bytes. This is a value,
/// not a native path, a resolved filesystem location, or terminal-safe text.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePath(String);

impl SourcePath {
    /// Validates a canonical path, then reserves and copies its bytes fallibly.
    ///
    /// The byte limit precedes structural validation. Within each component,
    /// the count limit precedes syntax checks, in path order.
    pub fn new(value: &str, limits: PathLimits) -> Result<Self, PathError> {
        validate_source(value, limits)?;
        copy_path(value).map(Self)
    }

    /// Checks the byte limit before decoding UTF-8 and validating the path.
    ///
    /// Invalid native encodings are never decoded lossily.
    pub fn from_bytes(value: &[u8], limits: PathLimits) -> Result<Self, PathError> {
        check_size(value.len(), limits)?;
        let text = std::str::from_utf8(value).map_err(|error| PathError::InvalidUtf8 {
            valid_up_to: error.valid_up_to(),
        })?;
        Self::new(text, limits)
    }

    /// Borrows the original canonical spelling. Escape it before rendering.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Iterates nonempty components without allocation or host path semantics.
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &str> + Clone {
        self.0.split('/')
    }
}

/// A canonical source path whose individual names pass portable output checks.
///
/// In addition to [`SourcePath`]'s grammar, rejects control characters, Windows
/// reserved punctuation and device names, trailing ASCII spaces or dots, and
/// components longer than 255 UTF-8 bytes (also bounding UTF-16 units to 255).
/// This does not check collisions, symlinks, containment, destination-specific
/// limits, or writability. Only the future filesystem executor can check these.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputPath(SourcePath);

impl OutputPath {
    /// Validates canonical structure and portable names before allocating.
    pub fn new(value: &str, limits: PathLimits) -> Result<Self, PathError> {
        validate_source(value, limits)?;
        validate_output(value)?;
        copy_path(value).map(|value| Self(SourcePath(value)))
    }

    /// Checks byte size, decodes UTF-8, then validates before allocating.
    pub fn from_bytes(value: &[u8], limits: PathLimits) -> Result<Self, PathError> {
        check_size(value.len(), limits)?;
        let text = std::str::from_utf8(value).map_err(|error| PathError::InvalidUtf8 {
            valid_up_to: error.valid_up_to(),
        })?;
        Self::new(text, limits)
    }

    /// Borrows the canonical spelling. Escape source-derived text for rendering.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Borrows the weaker source-path view without copying or revalidation.
    pub fn as_source(&self) -> &SourcePath {
        &self.0
    }

    /// Returns the source-path value without copying or allocation.
    pub fn into_source(self) -> SourcePath {
        self.0
    }
}

impl TryFrom<SourcePath> for OutputPath {
    type Error = PathError;

    /// Validates portable names and reuses the source path's storage and limits.
    fn try_from(value: SourcePath) -> Result<Self, Self::Error> {
        validate_output(value.as_str())?;
        Ok(Self(value))
    }
}

fn copy_path(value: &str) -> Result<String, PathError> {
    let mut result = String::new();
    result
        .try_reserve_exact(value.len())
        .map_err(|_| PathError::AllocationFailed)?;
    result.push_str(value);
    Ok(result)
}

fn check_size(bytes: usize, limits: PathLimits) -> Result<(), PathError> {
    if bytes > limits.max_path_bytes {
        return Err(PathError::TooLong {
            bytes,
            limit: limits.max_path_bytes,
        });
    }
    Ok(())
}

fn validate_source(value: &str, limits: PathLimits) -> Result<(), PathError> {
    check_size(value.len(), limits)?;
    if value.is_empty() {
        return Err(PathError::Empty);
    }
    if value.starts_with('/') {
        return Err(PathError::Absolute);
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(PathError::DrivePrefix);
    }
    let mut offset = 0;
    for (index, component) in value.split('/').enumerate() {
        if index >= limits.max_components {
            return Err(PathError::TooManyComponents {
                offset,
                limit: limits.max_components,
            });
        }
        if component.is_empty() {
            return Err(PathError::EmptyComponent { offset });
        }
        if matches!(component, "." | "..") {
            return Err(PathError::DotComponent { offset });
        }
        for (local, byte) in component.bytes().enumerate() {
            if byte == b'\\' {
                return Err(PathError::Backslash {
                    offset: offset + local,
                });
            }
            if byte == 0 {
                return Err(PathError::Nul {
                    offset: offset + local,
                });
            }
        }
        offset += component.len() + 1;
    }
    Ok(())
}

fn validate_output(value: &str) -> Result<(), PathError> {
    let mut offset = 0;
    for component in value.split('/') {
        if component.len() > 255 {
            return Err(PathError::OutputComponentTooLong {
                offset,
                bytes: component.len(),
            });
        }
        for (local, ch) in component.char_indices() {
            if ch.is_control() {
                return Err(PathError::OutputControl {
                    offset: offset + local,
                });
            }
            if matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                return Err(PathError::OutputCharacter {
                    offset: offset + local,
                });
            }
        }
        if component.ends_with([' ', '.']) {
            return Err(PathError::OutputEnding {
                offset: offset + component.len() - 1,
            });
        }
        if is_device(component) {
            return Err(PathError::OutputDevice { offset });
        }
        offset += component.len() + 1;
    }
    Ok(())
}

fn is_device(component: &str) -> bool {
    // Include extensions and spaces before an extension, which Win32 may alias.
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ');
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
    {
        return true;
    }
    if stem.as_bytes().get(..3).is_some_and(|prefix| {
        prefix.eq_ignore_ascii_case(b"COM") || prefix.eq_ignore_ascii_case(b"LPT")
    }) {
        return matches!(
            &stem[3..],
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        );
    }
    false
}
