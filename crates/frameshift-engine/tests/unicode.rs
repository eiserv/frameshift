//! Pinned Unicode identifier classification through the public facade.

use frameshift_engine::{UNICODE_VERSION, is_js_identifier_part, is_js_identifier_start};

#[test]
fn tables_match_the_pinned_unicode_totals_and_nest() {
    assert_eq!(UNICODE_VERSION, "18.0.0");
    let mut starts = 0;
    let mut parts = 0;
    for ch in char::MIN..=char::MAX {
        let start = is_js_identifier_start(ch);
        let part = is_js_identifier_part(ch);
        assert!(!start || part, "U+{:04X}", u32::from(ch));
        starts += usize::from(start);
        parts += usize::from(part);
    }
    // DerivedCoreProperties-18.0.0.txt totals, plus `$` and `_` for starts and
    // `$` for parts (`_` already has ID_Continue).
    assert_eq!(starts, 158_739 + 2);
    assert_eq!(parts, 162_100 + 1);
}

#[test]
fn ascii_follows_the_ecmascript_additions_exactly() {
    for byte in 0..=0x7f_u8 {
        let ch = char::from(byte);
        assert_eq!(
            is_js_identifier_start(ch),
            ch.is_ascii_alphabetic() || ch == '$' || ch == '_',
            "{byte:#04x}"
        );
        assert_eq!(
            is_js_identifier_part(ch),
            ch.is_ascii_alphanumeric() || ch == '$' || ch == '_',
            "{byte:#04x}"
        );
    }
}

#[test]
fn derived_property_edges_are_not_approximated() {
    for (ch, start, part) in [
        ('é', true, true),
        ('中', true, true),
        ('\u{3d000}', true, true),
        // Other_ID_Start symbols that are not letters.
        ('\u{2118}', true, true),
        ('\u{212e}', true, true),
        ('\u{309b}', true, true),
        // Other_ID_Continue and nonspacing marks, digits, and joiners.
        ('\u{b7}', false, true),
        ('\u{387}', false, true),
        ('\u{1369}', false, true),
        ('\u{19da}', false, true),
        ('\u{30fb}', false, true),
        ('\u{301}', false, true),
        ('\u{660}', false, true),
        ('\u{200c}', false, true),
        ('\u{200d}', false, true),
        // Pattern_Syntax letters, whitespace, format, and symbols.
        ('\u{2e2f}', false, false),
        ('\u{a0}', false, false),
        ('\u{feff}', false, false),
        ('\u{200b}', false, false),
        ('\u{2028}', false, false),
        ('€', false, false),
        ('🦀', false, false),
        ('\u{ffff}', false, false),
        ('\u{10ffff}', false, false),
    ] {
        assert_eq!(is_js_identifier_start(ch), start, "U+{:04X}", u32::from(ch));
        assert_eq!(is_js_identifier_part(ch), part, "U+{:04X}", u32::from(ch));
    }
}
