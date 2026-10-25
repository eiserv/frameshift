//! Thin command-line adapter for Frameshift's implemented engine commands.

mod analysis_cli;
mod args;
mod render;

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use args::{Format, InspectRequest, Request, UsageError};
use frameshift_engine::inspect::{InspectOptions, inspect};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let request = args::parse(&arguments);
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
        Ok(Request::Extra(request)) => analysis_cli::run(*request, stdout, stderr)?,
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
        Ok(Request::Inspect(request)) => run_inspect(&request, stdout, stderr)?,
        Err(error) => fail(stdout, stderr, error.format, 2, error.message)?,
    };
    stdout.flush()?;
    stderr.flush()?;
    Ok(code)
}

fn run_inspect(
    request: &InspectRequest,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<u8> {
    let options = InspectOptions {
        packages: request.packages.clone(),
        ..InspectOptions::default()
    };
    match inspect(Path::new(&request.source), &options) {
        Ok(inspection) => {
            match request.format {
                Format::Text => render::inspection(stdout, &inspection)?,
                Format::Json => stdout.write_all(inspection.document().as_bytes())?,
            }
            Ok(0)
        }
        // Engine errors describe categories, never paths or source text.
        Err(error) => {
            let code = if error.is_internal() { 70 } else { 3 };
            fail(stdout, stderr, request.format, code, &error.to_string())
        }
    }
}

fn fail(
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    format: Format,
    code: u8,
    message: &str,
) -> io::Result<u8> {
    match format {
        Format::Text => {}
        Format::Json => render::error_document(stdout, code, message)?,
    }
    writeln!(stderr, "error: {message}")?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_failures_are_returned_instead_of_reporting_success() {
        let mut output = io::Cursor::new([0_u8; 0]);
        let mut errors = Vec::new();
        assert!(run(Ok(Request::ShortVersion), &mut output, &mut errors).is_err());
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
        assert!(run(Ok(Request::ShortVersion), &mut FlushFailure, &mut errors).is_err());
        assert!(run(Ok(Request::ShortVersion), &mut errors, &mut FlushFailure).is_err());
    }
}
