//! The path target: arbitrary bytes and limits through the public foundation API.

use frameshift_engine::foundation::{OutputPath, PathError, PathLimits, SourcePath};

/// Checks repetition, an independent acceptance model, spelling, and conversion.
pub fn run(input: &[u8]) -> Result<(), String> {
    for limits in [
        PathLimits::default(),
        PathLimits {
            max_path_bytes: input.len() / 2,
            max_components: 2,
        },
        PathLimits {
            max_path_bytes: input.len(),
            max_components: usize::from(input.first().copied().unwrap_or(0) % 4),
        },
    ] {
        check(input, limits)?;
    }
    Ok(())
}

fn check(input: &[u8], limits: PathLimits) -> Result<(), String> {
    let source = SourcePath::from_bytes(input, limits);
    let output = OutputPath::from_bytes(input, limits);
    if source != SourcePath::from_bytes(input, limits)
        || output != OutputPath::from_bytes(input, limits)
    {
        return Err("repeated path validation changed its result".to_owned());
    }
    if input.len() > limits.max_path_bytes {
        let expected = PathError::TooLong {
            bytes: input.len(),
            limit: limits.max_path_bytes,
        };
        if source != Err(expected) || output != Err(expected) {
            return Err("path byte limit did not precede decoding and syntax".to_owned());
        }
        return Ok(());
    }
    let text = match std::str::from_utf8(input) {
        Ok(text) => text,
        Err(error) => {
            let expected = PathError::InvalidUtf8 {
                valid_up_to: error.valid_up_to(),
            };
            if source != Err(expected) || output != Err(expected) {
                return Err("invalid UTF-8 was not rejected exactly".to_owned());
            }
            return Ok(());
        }
    };
    if source != SourcePath::new(text, limits) || output != OutputPath::new(text, limits) {
        return Err("byte and text path constructors disagree".to_owned());
    }
    // A declarative reference grammar, independent of error precedence or offsets.
    let components: Vec<_> = text.split('/').collect();
    let drive = text.as_bytes().get(1) == Some(&b':')
        && text.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    let canonical = !text.is_empty()
        && !drive
        && !text.contains(['\\', '\0'])
        && components.len() <= limits.max_components
        && components
            .iter()
            .all(|part| !matches!(*part, "" | "." | ".."));
    if source.is_ok() != canonical {
        return Err("source path disagrees with the reference grammar".to_owned());
    }
    let portable = canonical && components.iter().all(|part| portable_component(part));
    if output.is_ok() != portable {
        return Err("output path disagrees with the portable-name model".to_owned());
    }
    if let Ok(source) = source {
        if source.as_str().as_bytes() != input
            || source.components().collect::<Vec<_>>() != components
        {
            return Err("source path spelling or components changed".to_owned());
        }
        let strengthened = OutputPath::try_from(source.clone());
        if strengthened != output {
            return Err("strengthening a source path changed validation".to_owned());
        }
        if let Ok(output) = strengthened
            && (output.as_source() != &source || output.into_source() != source)
        {
            return Err("weakening an output path changed the source".to_owned());
        }
    }
    Ok(())
}

fn portable_component(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let fixed_device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str());
    let numbered_device = ["COM", "LPT"].iter().any(|prefix| {
        ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"]
            .iter()
            .any(|digit| stem == format!("{prefix}{digit}"))
    });
    part.len() <= 255
        && !part.chars().any(|ch| {
            (ch as u32) < 32 || (0x7f..=0x9f).contains(&(ch as u32)) || "<>:\"|?*".contains(ch)
        })
        && !part.ends_with(['.', ' '])
        && !fixed_device
        && !numbered_device
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_paths_and_malformed_bytes_satisfy_the_contracts() {
        let names = [
            "ok", "é", "e\u{301}", ".", "..", "", "CON", "coM².ts", "AUX .txt", "space ",
            "ads:name", "a\\b", "\0", "\u{9f}", "🦀",
        ];
        for first in names {
            for last in names {
                run(format!("{first}/{last}").as_bytes()).unwrap();
            }
        }
        for byte in 0..=255 {
            run(&[b'a', b'/', byte]).unwrap();
        }
        for count in [255, 256, 257, 32_768, 32_769] {
            run(&vec![b'a'; count]).unwrap();
        }
    }
}
