//! Names accepted by `\p{...}` and `\P{...}` in Unicode-mode patterns
//! (ECMAScript section 22.2.2.9, tables 65 to 67). Values of General_Category
//! and Script come from the pinned Unicode tables; the property name tables
//! below are part of the ECMAScript specification, not of the UCD.

use crate::unicode_tables::{GENERAL_CATEGORY_VALUES, SCRIPT_VALUES};

/// What a valid property expression denotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PropertyKind {
    /// A set of code points.
    CodePoints,
    /// A binary property of strings, only valid in UnicodeSets mode.
    Strings,
}

// Table 66, checked on 2026-10-12: binary property names and aliases.
const BINARY_PROPERTIES: &[&str] = &[
    "AHex",
    "ASCII",
    "ASCII_Hex_Digit",
    "Alpha",
    "Alphabetic",
    "Any",
    "Assigned",
    "Bidi_C",
    "Bidi_Control",
    "Bidi_M",
    "Bidi_Mirrored",
    "CI",
    "CWCF",
    "CWCM",
    "CWKCF",
    "CWL",
    "CWT",
    "CWU",
    "Case_Ignorable",
    "Cased",
    "Changes_When_Casefolded",
    "Changes_When_Casemapped",
    "Changes_When_Lowercased",
    "Changes_When_NFKC_Casefolded",
    "Changes_When_Titlecased",
    "Changes_When_Uppercased",
    "DI",
    "Dash",
    "Default_Ignorable_Code_Point",
    "Dep",
    "Deprecated",
    "Dia",
    "Diacritic",
    "EBase",
    "EComp",
    "EMod",
    "EPres",
    "Emoji",
    "Emoji_Component",
    "Emoji_Modifier",
    "Emoji_Modifier_Base",
    "Emoji_Presentation",
    "Ext",
    "ExtPict",
    "Extended_Pictographic",
    "Extender",
    "Gr_Base",
    "Gr_Ext",
    "Grapheme_Base",
    "Grapheme_Extend",
    "Hex",
    "Hex_Digit",
    "IDC",
    "IDS",
    "IDSB",
    "IDST",
    "IDS_Binary_Operator",
    "IDS_Trinary_Operator",
    "ID_Continue",
    "ID_Start",
    "Ideo",
    "Ideographic",
    "Join_C",
    "Join_Control",
    "LOE",
    "Logical_Order_Exception",
    "Lower",
    "Lowercase",
    "Math",
    "NChar",
    "Noncharacter_Code_Point",
    "Pat_Syn",
    "Pat_WS",
    "Pattern_Syntax",
    "Pattern_White_Space",
    "QMark",
    "Quotation_Mark",
    "RI",
    "Radical",
    "Regional_Indicator",
    "SD",
    "STerm",
    "Sentence_Terminal",
    "Soft_Dotted",
    "Term",
    "Terminal_Punctuation",
    "UIdeo",
    "Unified_Ideograph",
    "Upper",
    "Uppercase",
    "VS",
    "Variation_Selector",
    "White_Space",
    "XIDC",
    "XIDS",
    "XID_Continue",
    "XID_Start",
    "space",
];

// Table 67, checked on 2026-10-12: binary properties of strings.
const STRING_PROPERTIES: &[&str] = &[
    "Basic_Emoji",
    "Emoji_Keycap_Sequence",
    "RGI_Emoji",
    "RGI_Emoji_Flag_Sequence",
    "RGI_Emoji_Modifier_Sequence",
    "RGI_Emoji_Tag_Sequence",
    "RGI_Emoji_ZWJ_Sequence",
];

// Lookups binary search byte-wise; an unsorted table would silently reject names.
const _: () = assert!(sorted_unique(BINARY_PROPERTIES));
const _: () = assert!(sorted_unique(STRING_PROPERTIES));
const _: () = assert!(sorted_unique(GENERAL_CATEGORY_VALUES));
const _: () = assert!(sorted_unique(SCRIPT_VALUES));

/// Classifies `name=value` (table 65) or a lone name or value. Matching is
/// exact and case-sensitive, as ECMAScript requires. `None` means the
/// expression names no property and is a syntax error.
pub(crate) fn classify(name: &str, value: Option<&str>) -> Option<PropertyKind> {
    let Some(value) = value else {
        if contains(GENERAL_CATEGORY_VALUES, name) || contains(BINARY_PROPERTIES, name) {
            return Some(PropertyKind::CodePoints);
        }
        return contains(STRING_PROPERTIES, name).then_some(PropertyKind::Strings);
    };
    let values = match name {
        "General_Category" | "gc" => GENERAL_CATEGORY_VALUES,
        "Script" | "sc" | "Script_Extensions" | "scx" => SCRIPT_VALUES,
        _ => return None,
    };
    contains(values, value).then_some(PropertyKind::CodePoints)
}

fn contains(table: &[&str], name: &str) -> bool {
    table.binary_search(&name).is_ok()
}

const fn sorted_unique(table: &[&str]) -> bool {
    let mut index = 1;
    while index < table.len() {
        if !less(table[index - 1].as_bytes(), table[index].as_bytes()) {
            return false;
        }
        index += 1;
    }
    true
}

const fn less(left: &[u8], right: &[u8]) -> bool {
    let mut index = 0;
    while index < left.len() && index < right.len() {
        if left[index] != right[index] {
            return left[index] < right[index];
        }
        index += 1;
    }
    left.len() < right.len()
}
