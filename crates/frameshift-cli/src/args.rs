//! Strict parsing of the implemented command contract, without project access.

use std::ffi::{OsStr, OsString};

use frameshift_engine::foundation::{PathLimits, SourcePath};
use frameshift_engine::inspect::PackageScope;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HelpTopic {
    Root,
    Help,
    Inspect,
    Version,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InspectRequest {
    /// The native source path, which need not be Unicode.
    pub(crate) source: OsString,
    pub(crate) packages: PackageScope,
    pub(crate) format: Format,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Request {
    Help(HelpTopic),
    Version(Format),
    ShortVersion,
    Inspect(InspectRequest),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UsageError {
    pub(crate) format: Format,
    pub(crate) message: &'static str,
}

const ENCODING: &str = "command arguments must be valid UTF-8";

// Detect an explicit JSON request even when an earlier argument is invalid.
// Error output never copies user-supplied strings into the terminal.
fn error_format(args: &[Option<&str>]) -> Format {
    if args.contains(&Some("--format=json"))
        || args
            .windows(2)
            .any(|pair| pair[0] == Some("--format") && pair[1] == Some("json"))
    {
        Format::Json
    } else {
        Format::Text
    }
}

/// The index of the `inspect` source operand, the only argument that may
/// be a non-Unicode native path. Option values are skipped as the parser
/// consumes them.
fn source_operand(args: &[Option<&str>]) -> Option<usize> {
    let mut words = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            Some("--format" | "--package") => index += 1,
            Some(value) if value.starts_with('-') => {}
            _ => words.push(index),
        }
        index += 1;
    }
    match words.as_slice() {
        [command, source, ..] if args[*command] == Some("inspect") => Some(*source),
        _ => None,
    }
}

pub(crate) fn parse(native: &[OsString]) -> Result<Request, UsageError> {
    let args: Vec<Option<&str>> = native.iter().map(|arg| arg.to_str()).collect();
    let error = |message| UsageError {
        format: error_format(&args),
        message,
    };
    let source = source_operand(&args);
    if args
        .iter()
        .enumerate()
        .any(|(index, arg)| arg.is_none() && Some(index) != source)
    {
        return Err(error(ENCODING));
    }
    let mut format = None;
    let mut package = None;
    let mut help = false;
    let mut short_version = false;
    let mut words: Vec<&OsStr> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        index += 1;
        let Some(text) = arg else {
            words.push(&native[index - 1]);
            continue;
        };
        match text {
            "--help" | "-h" => {
                if help {
                    return Err(error("help option may be supplied only once"));
                }
                help = true;
            }
            "--version" => {
                if short_version {
                    return Err(error("version option may be supplied only once"));
                }
                short_version = true;
            }
            "--format" => {
                let value = args
                    .get(index)
                    .copied()
                    .flatten()
                    .ok_or_else(|| error("--format requires text or json"))?;
                index += 1;
                set_format(&mut format, value).map_err(error)?;
            }
            value if value.starts_with("--format=") => {
                set_format(&mut format, &value[9..]).map_err(error)?;
            }
            "--package" => {
                let value = args
                    .get(index)
                    .copied()
                    .flatten()
                    .ok_or_else(|| error(PACKAGE_VALUE))?;
                index += 1;
                set_package(&mut package, value).map_err(error)?;
            }
            value if value.starts_with("--package=") => {
                set_package(&mut package, &value[10..]).map_err(error)?;
            }
            value if value.starts_with('-') => {
                return Err(error("unknown option; run 'frameshift help' for usage"));
            }
            _ => {
                if words.len() == 2 {
                    return Err(error("too many command arguments"));
                }
                words.push(&native[index - 1]);
            }
        }
    }
    if short_version {
        if help || !words.is_empty() || format.is_some() || package.is_some() {
            return Err(error("--version cannot be combined with other arguments"));
        }
        return Ok(Request::ShortVersion);
    }
    let command: Vec<Option<&str>> = words.iter().map(|word| word.to_str()).collect();
    let topic = match command.as_slice() {
        [] => HelpTopic::Root,
        [Some("help")] => HelpTopic::Root,
        [Some("help"), Some("help")] => HelpTopic::Help,
        [Some("help"), Some("version")] => HelpTopic::Version,
        [Some("help"), Some("inspect")] => HelpTopic::Inspect,
        [Some("version")] if !help => {
            if package.is_some() {
                return Err(error(PACKAGE_ONLY));
            }
            return Ok(Request::Version(format.unwrap_or(Format::Text)));
        }
        [Some("version")] => HelpTopic::Version,
        [Some("inspect"), ..] if !help => {
            return Ok(Request::Inspect(InspectRequest {
                source: words.get(1).map_or_else(|| ".".into(), |&word| word.into()),
                packages: package.unwrap_or_default(),
                format: format.unwrap_or(Format::Text),
            }));
        }
        [Some("inspect")] => HelpTopic::Inspect,
        [Some("inspect"), _] => return Err(error("help for inspect takes no source")),
        [_] | [_, _] => {
            return Err(error(
                "unknown or unavailable command; run 'frameshift help'",
            ));
        }
        _ => return Err(error("too many command arguments")),
    };
    if format.is_some() {
        return Err(error(
            "--format is available only for the inspect and version commands",
        ));
    }
    if package.is_some() {
        return Err(error(PACKAGE_ONLY));
    }
    Ok(Request::Help(topic))
}

const PACKAGE_VALUE: &str =
    "--package requires a directory relative to the source with / separators, or .";
const PACKAGE_ONLY: &str = "--package is available only for the inspect command";

fn set_format(format: &mut Option<Format>, value: &str) -> Result<(), &'static str> {
    if format.is_some() {
        return Err("--format may be supplied only once");
    }
    *format = Some(match value {
        "text" => Format::Text,
        "json" => Format::Json,
        _ => return Err("--format requires text or json"),
    });
    Ok(())
}

fn set_package(package: &mut Option<PackageScope>, value: &str) -> Result<(), &'static str> {
    if package.is_some() {
        return Err("--package may be supplied only once");
    }
    // The value names a canonical path, never a native one: a backslash, a
    // trailing slash, or `..` is rejected rather than normalized (D-054).
    *package = Some(if value == "." {
        PackageScope::Root
    } else {
        PackageScope::Member(
            SourcePath::new(value, PathLimits::default()).map_err(|_| PACKAGE_VALUE)?,
        )
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_text(args: &[&str]) -> Result<Request, UsageError> {
        let native: Vec<OsString> = args.iter().map(OsString::from).collect();
        parse(&native)
    }

    #[test]
    fn inspect_takes_an_optional_source_package_and_format() {
        let request = |source: &str, packages, format| {
            Ok(Request::Inspect(InspectRequest {
                source: source.into(),
                packages,
                format,
            }))
        };
        assert_eq!(
            parse_text(&["inspect"]),
            request(".", PackageScope::All, Format::Text)
        );
        assert_eq!(
            parse_text(&["--format=json", "inspect", "site", "--package", "."]),
            request("site", PackageScope::Root, Format::Json)
        );
        let member = SourcePath::new("apps/web", PathLimits::default()).unwrap();
        assert_eq!(
            parse_text(&["inspect", "--package=apps/web", "site"]),
            request("site", PackageScope::Member(member), Format::Text)
        );
        assert_eq!(
            parse_text(&["inspect", "--help"]),
            Ok(Request::Help(HelpTopic::Inspect))
        );
        assert_eq!(
            parse_text(&["help", "inspect"]),
            Ok(Request::Help(HelpTopic::Inspect))
        );
    }

    #[test]
    fn inspect_rejects_invalid_packages_and_misplaced_options() {
        for args in [
            &["inspect", "--package", "apps\\web"][..],
            &["inspect", "--package", "apps/web/"],
            &["inspect", "--package", "../web"],
            &["inspect", "--package", ""],
            &["inspect", "--package"],
            &["inspect", "--package", ".", "--package", "."],
            &["inspect", "a", "b"],
            &["inspect", "a", "--help"],
            &["version", "--package", "."],
            &["help", "--package", "."],
            &["--version", "--package", "."],
        ] {
            let error = parse_text(args).unwrap_err();
            assert_eq!(error.format, Format::Text, "{args:?}");
        }
        assert_eq!(
            parse_text(&["inspect", "--format", "json", "--package", "a\\b"])
                .unwrap_err()
                .format,
            Format::Json
        );
    }

    #[cfg(unix)]
    #[test]
    fn only_the_inspect_source_may_be_a_non_unicode_path() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = OsString::from_vec(vec![b's', 0xff]);
        assert_eq!(
            parse(&["inspect".into(), invalid.clone()]),
            Ok(Request::Inspect(InspectRequest {
                source: invalid.clone(),
                packages: PackageScope::All,
                format: Format::Text,
            }))
        );
        for args in [
            vec![invalid.clone()],
            vec!["inspect".into(), ".".into(), invalid.clone()],
            vec!["inspect".into(), "--package".into(), invalid.clone()],
            vec!["version".into(), invalid],
        ] {
            assert_eq!(parse(&args).unwrap_err().message, ENCODING);
        }
    }
}
