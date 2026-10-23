//! ECMAScript number semantics that evaluation needs: `Number::toString`
//! with radix 10, `ToNumber` of strings, and numeric literal values.

use frameshift_syntax::NumericBase;

/// `Number::toString(x)` for radix 10 (ECMA-262 6.1.6.1.20).
pub(crate) fn to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x == 0.0 {
        return "0".to_owned();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if x < 0.0 {
        return format!("-{}", to_string(-x));
    }
    // Rust's exponent format prints the shortest digits that round-trip,
    // which are the digits the specification selects.
    let formatted = format!("{x:e}");
    let (mantissa, exponent) = formatted.split_once('e').expect("exponent format");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i64;
    let n = exponent.parse::<i64>().expect("exponent") + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let sign = if n >= 1 { '+' } else { '-' };
        let rest = if k > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!("{}{rest}e{sign}{}", &digits[..1], (n - 1).abs())
    }
}

/// The value of a numeric literal spelling, or `None` beyond what this
/// conversion represents exactly.
pub(crate) fn literal(spelling: &str, base: NumericBase, legacy_octal: bool) -> Option<f64> {
    let text: String = spelling.chars().filter(|&c| c != '_').collect();
    let integer = |digits: &str, radix: u32| -> Option<f64> {
        let value = u128::from_str_radix(digits, radix).ok()?;
        // Values beyond 2^53 lose precision as JavaScript rounds them.
        Some(value as f64)
    };
    match base {
        NumericBase::Binary => integer(&text[2..], 2),
        NumericBase::Octal => integer(&text[2..], 8),
        NumericBase::Hexadecimal => integer(&text[2..], 16),
        NumericBase::Decimal if legacy_octal => {
            if text.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
                integer(&text, 8)
            } else {
                text.parse().ok()
            }
        }
        NumericBase::Decimal => text.parse().ok(),
    }
}

/// `ToNumber` of a string (StringToNumber), for the forms that configuration
/// values use; anything else is `NaN`.
pub(crate) fn from_string(text: &str) -> f64 {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
    if trimmed.is_empty() {
        return 0.0;
    }
    let prefixed = |prefix: [&str; 2], radix| {
        prefix
            .iter()
            .find_map(|p| trimmed.strip_prefix(p))
            .map(|digits| {
                u128::from_str_radix(digits, radix)
                    .ok()
                    .filter(|_| !digits.is_empty())
                    .map_or(f64::NAN, |v| v as f64)
            })
    };
    if let Some(v) = prefixed(["0x", "0X"], 16)
        .or_else(|| prefixed(["0o", "0O"], 8))
        .or_else(|| prefixed(["0b", "0B"], 2))
    {
        return v;
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // StrDecimalLiteral admits no `inf`, `nan`, or underscores.
    if trimmed
        .bytes()
        .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
    {
        trimmed.parse().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_follows_the_specification() {
        for (x, s) in [
            (1.0, "1"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (123.456, "123.456"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (2.0f64.powi(53), "9007199254740992"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
            (-2.5e30, "-2.5e+30"),
        ] {
            assert_eq!(to_string(x), s, "{x}");
        }
    }

    #[test]
    fn strings_and_literals_convert() {
        assert_eq!(from_string(" 42 "), 42.0);
        assert_eq!(from_string(""), 0.0);
        assert_eq!(from_string("0x1F"), 31.0);
        assert!(from_string("1_0").is_nan());
        assert!(from_string("inf").is_nan());
        assert_eq!(literal("0b101", NumericBase::Binary, false), Some(5.0));
        assert_eq!(literal("017", NumericBase::Decimal, true), Some(15.0));
        assert_eq!(literal("019", NumericBase::Decimal, true), Some(19.0));
        assert_eq!(
            literal("1_000.5", NumericBase::Decimal, false),
            Some(1000.5)
        );
    }
}
