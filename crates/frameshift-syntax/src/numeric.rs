use frameshift_core::{SourceText, Span};

use crate::lexical::{
    LegacyOctal, LexicalError, LexicalErrorKind, LexicalLimits, error, span, validate,
};
use crate::unicode::is_js_identifier_start;

/// The spelling base of a numeric literal, without evaluating its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericBase {
    /// Decimal integer, fraction, or exponent syntax.
    Decimal,
    /// An integer with a `0b` or `0B` prefix.
    Binary,
    /// An integer with a `0o` or `0O` prefix.
    Octal,
    /// An integer with a `0x` or `0X` prefix.
    Hexadecimal,
}

impl NumericBase {
    fn contains(self, byte: u8) -> bool {
        match self {
            Self::Decimal => byte.is_ascii_digit(),
            Self::Binary => matches!(byte, b'0' | b'1'),
            Self::Octal => matches!(byte, b'0'..=b'7'),
            Self::Hexadecimal => byte.is_ascii_hexdigit(),
        }
    }
}

/// The ECMAScript value category indicated by the literal's spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericKind {
    /// A Number literal. No floating-point value is computed.
    Number,
    /// A BigInt literal ending in `n`. No integer value is computed.
    BigInt,
}

/// One original-source numeric literal, with its lexical category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumericLiteral {
    /// Exact source span, including separators, radix prefix, and BigInt suffix.
    pub span: Span,
    /// Base indicated by the original spelling. A legacy `017` is octal.
    pub base: NumericBase,
    /// Number or BigInt, without evaluation or normalization.
    pub kind: NumericKind,
    /// Whether the spelling is a `LegacyOctalIntegerLiteral` (`017`, octal
    /// base) or a `NonOctalDecimalIntegerLiteral` (`089`, decimal base,
    /// possibly with a fraction or exponent), which strict mode code forbids.
    pub legacy_octal: bool,
}

/// Scans one ECMAScript numeric literal at a parser-selected offset.
/// Supports decimal fractions and exponents, binary/octal/hexadecimal integers,
/// separators, and BigInt suffixes. Leading signs belong to separate operators.
/// Legacy leading-zero integers are rejected with [`LegacyOctal::Reject`], as
/// strict mode code requires, and marked in the result with
/// [`LegacyOctal::Permit`]; they never take separators or a BigInt suffix. A
/// directly following `IdentifierStartChar` (pinned Unicode `ID_Start`, `$`, or
/// `_`), backslash, or extra decimal digit is rejected; any other scalar ends
/// the literal. The caller must select JavaScript context, never JSX text or
/// literal bodies. This does not validate the surrounding expression or file.
/// Scanning is iterative, allocation-free, and linear in consumed bytes.
pub fn scan_js_number(
    source: &SourceText,
    offset: usize,
    legacy: LegacyOctal,
    limits: LexicalLimits,
) -> Result<NumericLiteral, LexicalError> {
    validate(source, offset)?;
    let bytes = source.as_str().as_bytes();
    let first = bytes.get(offset).copied();
    if !first.is_some_and(|byte| byte.is_ascii_digit())
        && !(first == Some(b'.') && bytes.get(offset + 1).is_some_and(u8::is_ascii_digit))
    {
        return Err(error(offset, LexicalErrorKind::ExpectedNumber));
    }
    let mut scan = NumberScan {
        bytes,
        start: offset,
        cursor: offset,
        max_bytes: limits.max_number_bytes,
    };
    let mut base = if first == Some(b'0') {
        match bytes.get(offset + 1) {
            Some(b'b' | b'B') => NumericBase::Binary,
            Some(b'o' | b'O') => NumericBase::Octal,
            Some(b'x' | b'X') => NumericBase::Hexadecimal,
            _ => NumericBase::Decimal,
        }
    } else {
        NumericBase::Decimal
    };
    let mut integer = true;
    let mut legacy_octal = false;
    match base {
        NumericBase::Binary | NumericBase::Octal | NumericBase::Hexadecimal => {
            scan.advance()?;
            scan.advance()?;
            scan.digits(base, true)?;
        }
        NumericBase::Decimal => {
            let mut fraction_allowed = true;
            if first == Some(b'.') {
                integer = false;
                fraction_allowed = false;
                scan.advance()?;
                scan.digits(base, true)?;
            } else if first == Some(b'0') {
                scan.advance()?;
                if scan.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    if legacy == LegacyOctal::Reject {
                        return Err(error(scan.cursor, LexicalErrorKind::LegacyLeadingZero));
                    }
                    legacy_octal = true;
                    let mut octal = true;
                    while let Some(byte) = scan.peek().filter(u8::is_ascii_digit) {
                        octal &= byte <= b'7';
                        scan.advance()?;
                    }
                    // `017` is a LegacyOctalIntegerLiteral and cannot take a
                    // fraction or exponent; `089` is a decimal literal.
                    if octal {
                        base = NumericBase::Octal;
                        fraction_allowed = false;
                    }
                }
                if scan.peek() == Some(b'_') {
                    return Err(error(
                        scan.cursor,
                        LexicalErrorKind::InvalidNumericSeparator,
                    ));
                }
            } else {
                scan.digits(base, true)?;
            }
            if fraction_allowed && scan.peek() == Some(b'.') {
                integer = false;
                scan.advance()?;
                scan.digits(base, false)?;
            }
            if base == NumericBase::Decimal && matches!(scan.peek(), Some(b'e' | b'E')) {
                integer = false;
                scan.advance()?;
                if matches!(scan.peek(), Some(b'+' | b'-')) {
                    scan.advance()?;
                }
                scan.digits(base, true)?;
            }
        }
    }
    let kind = if scan.peek() == Some(b'n') {
        if !integer || legacy_octal {
            return Err(error(
                scan.cursor,
                LexicalErrorKind::InvalidNumericContinuation,
            ));
        }
        scan.advance()?;
        NumericKind::BigInt
    } else {
        NumericKind::Number
    };
    // A backslash after a number could only begin an escaped identifier start,
    // which ECMAScript forbids there just like an unescaped one.
    if let Some(ch) = source.as_str()[scan.cursor..].chars().next()
        && (ch.is_ascii_digit() || ch == '\\' || is_js_identifier_start(ch))
    {
        return Err(error(
            scan.cursor,
            LexicalErrorKind::InvalidNumericContinuation,
        ));
    }
    Ok(NumericLiteral {
        span: span(offset, scan.cursor)?,
        base,
        kind,
        legacy_octal,
    })
}

struct NumberScan<'a> {
    bytes: &'a [u8],
    start: usize,
    cursor: usize,
    max_bytes: usize,
}

impl NumberScan<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn advance(&mut self) -> Result<(), LexicalError> {
        if self.cursor - self.start >= self.max_bytes {
            return Err(error(self.cursor, LexicalErrorKind::NumberLimit));
        }
        self.cursor += 1;
        Ok(())
    }

    fn digits(&mut self, base: NumericBase, required: bool) -> Result<(), LexicalError> {
        let mut has_digit = false;
        while let Some(byte) = self.peek() {
            if base.contains(byte) {
                self.advance()?;
                has_digit = true;
            } else if byte == b'_' {
                if !has_digit
                    || !self
                        .bytes
                        .get(self.cursor + 1)
                        .is_some_and(|&next| base.contains(next))
                {
                    return Err(error(
                        self.cursor,
                        LexicalErrorKind::InvalidNumericSeparator,
                    ));
                }
                self.advance()?;
                has_digit = false;
            } else {
                break;
            }
        }
        if required && !has_digit {
            return Err(error(self.cursor, LexicalErrorKind::MissingNumericDigit));
        }
        Ok(())
    }
}
