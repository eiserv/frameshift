//! Strict parsing for the implemented CLI shell, without project access.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HelpTopic {
    Root,
    Help,
    Version,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Request {
    Help(HelpTopic),
    Version(Format),
    ShortVersion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UsageError {
    pub(crate) format: Format,
    pub(crate) message: &'static str,
}

// Detect an explicit JSON request even when an earlier argument is invalid.
// Error output never copies user-supplied strings into the terminal.
fn error_format(args: &[String]) -> Format {
    if args.iter().any(|arg| arg == "--format=json")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--format" && pair[1] == "json")
    {
        Format::Json
    } else {
        Format::Text
    }
}

pub(crate) fn parse(args: &[String]) -> Result<Request, UsageError> {
    let error = |message| UsageError {
        format: error_format(args),
        message,
    };
    let mut format = None;
    let mut help = false;
    let mut short_version = false;
    let mut words = Vec::new();
    let mut arguments = args.iter();
    while let Some(arg) = arguments.next() {
        match arg.as_str() {
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
                let value = arguments
                    .next()
                    .ok_or_else(|| error("--format requires text or json"))?;
                set_format(&mut format, value).map_err(error)?;
            }
            value if value.starts_with("--format=") => {
                set_format(&mut format, &value[9..]).map_err(error)?;
            }
            value if value.starts_with('-') => {
                return Err(error("unknown option; run 'frameshift help' for usage"));
            }
            value => {
                if words.len() == 2 {
                    return Err(error("too many command arguments"));
                }
                words.push(value);
            }
        }
    }
    if short_version {
        if help || !words.is_empty() || format.is_some() {
            return Err(error("--version cannot be combined with other arguments"));
        }
        return Ok(Request::ShortVersion);
    }
    let topic = match words.as_slice() {
        [] => HelpTopic::Root,
        ["help"] => HelpTopic::Root,
        ["help", "help"] => HelpTopic::Help,
        ["help", "version"] => HelpTopic::Version,
        ["version"] if !help => return Ok(Request::Version(format.unwrap_or(Format::Text))),
        ["version"] => HelpTopic::Version,
        [_] | [_, _] => {
            return Err(error(
                "unknown or unavailable command; run 'frameshift help'",
            ));
        }
        _ => return Err(error("too many command arguments")),
    };
    if format.is_some() {
        return Err(error("--format is available only for the version command"));
    }
    Ok(Request::Help(topic))
}

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
