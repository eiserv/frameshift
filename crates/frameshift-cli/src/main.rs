//! Thin command-line adapter for Frameshift's implemented engine shell.

mod args;
mod render;

use std::io::{self, Write};
use std::process::ExitCode;

use args::{Format, Request, UsageError};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let mut strings = Vec::with_capacity(arguments.len());
    let mut invalid_encoding = false;
    for argument in arguments {
        match argument.into_string() {
            Ok(value) => strings.push(value),
            Err(_) => {
                invalid_encoding = true;
                // Preserve option/value adjacency when deciding error format.
                strings.push(String::new());
            }
        }
    }
    let request = if invalid_encoding {
        let format = match args::parse(&strings) {
            Ok(Request::Version(format)) | Err(UsageError { format, .. }) => format,
            Ok(Request::Help(_)) | Ok(Request::ShortVersion) => Format::Text,
        };
        Err(UsageError {
            format,
            message: "command arguments must be valid UTF-8",
        })
    } else {
        args::parse(&strings)
    };
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    match run(request, &mut stdout, &mut stderr) {
        Ok(code) => ExitCode::from(code),
        Err(_) => {
            let _ = stderr.write_all(b"error: cannot write command output\n");
            ExitCode::from(6)
        }
    }
}

fn run(
    request: Result<Request, UsageError>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<u8> {
    let code = match request {
        Ok(Request::Help(topic)) => {
            stdout.write_all(render::help(topic).as_bytes())?;
            0
        }
        Ok(Request::Version(format)) => {
            render::version(stdout, format, frameshift_engine::version::info())?;
            0
        }
        Ok(Request::ShortVersion) => {
            writeln!(
                stdout,
                "frameshift {}",
                frameshift_engine::version::info().engine_version
            )?;
            0
        }
        Err(error) => {
            match error.format {
                Format::Text => {}
                Format::Json => render::usage_error(stdout, error.message)?,
            }
            writeln!(stderr, "error: {}", error.message)?;
            2
        }
    };
    stdout.flush()?;
    stderr.flush()?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_failures_are_returned_instead_of_reporting_success() {
        let mut output = io::Cursor::new([0_u8; 0]);
        let mut errors = Vec::new();
        let request = Ok(Request::ShortVersion);
        assert!(run(request, &mut output, &mut errors).is_err());
        let error = Err(UsageError {
            format: Format::Text,
            message: "invalid command",
        });
        assert!(run(error, &mut errors, &mut output).is_err());

        struct FlushFailure;
        impl Write for FlushFailure {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("test flush failure"))
            }
        }
        assert!(run(request, &mut FlushFailure, &mut errors).is_err());
        assert!(run(request, &mut errors, &mut FlushFailure).is_err());
    }
}
