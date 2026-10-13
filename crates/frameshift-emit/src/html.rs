//! HTML element and attribute facts and escaping for Astro templates.

/// Escapes text content. `&`, `<`, and `>` are HTML-significant, `{` and `}`
/// open and close Astro expressions, tab and line breaks would be subject to
/// Astro's JSX-style whitespace handling, and U+00A0 is spelled `&nbsp;` so
/// that it stays visible in the output.
pub(crate) fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '{' => out.push_str("&#123;"),
            '}' => out.push_str("&#125;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            ch => out.push(ch),
        }
    }
    out
}

/// Escapes a double-quoted attribute value. Astro passes quoted values to
/// HTML unchanged, including `{` and `}`.
pub(crate) fn escape_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            ch => out.push(ch),
        }
    }
    out
}

/// HTML void elements, which have no end tag and no children.
pub(crate) fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Elements whose whitespace Astro preserves, so the printer adds none.
pub(crate) fn is_whitespace_sensitive(name: &str) -> bool {
    matches!(name, "pre" | "textarea" | "listing")
}

/// Raw-text and escapable raw-text elements, whose JSX children follow
/// different rules in React and are not converted.
pub(crate) fn is_raw_text_element(name: &str) -> bool {
    matches!(name, "script" | "style" | "textarea" | "title" | "noscript")
}

/// Root elements of foreign content, where attribute names are case-sensitive
/// and React maps camelCase props differently. Not converted yet.
pub(crate) fn is_foreign_root(name: &str) -> bool {
    matches!(name, "svg" | "math")
}

/// HTML boolean attributes: React renders `true` as an empty value, so a
/// valueless JSX attribute becomes a valueless HTML attribute.
pub(crate) fn is_boolean_attribute(name: &str) -> bool {
    matches!(
        name,
        "allowfullscreen"
            | "async"
            | "autofocus"
            | "autoplay"
            | "checked"
            | "controls"
            | "default"
            | "defer"
            | "disabled"
            | "formnovalidate"
            | "hidden"
            | "inert"
            | "itemscope"
            | "loop"
            | "multiple"
            | "muted"
            | "nomodule"
            | "novalidate"
            | "open"
            | "playsinline"
            | "readonly"
            | "required"
            | "reversed"
            | "selected"
    )
}

/// What React renders for an attribute name of an HTML element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HtmlAttribute {
    /// The attribute is rendered under this HTML name.
    Rename(&'static str),
    /// The name is rendered unchanged.
    Same,
    /// React renders nothing for the prop.
    Drop,
    /// `dangerouslySetInnerHTML`.
    InnerHtml,
    /// `style`.
    Style,
    /// `key`, which React consumes.
    Key,
    /// A prop without a verified HTML mapping.
    Unsupported,
}

/// Maps a React prop name on an HTML element to its rendered attribute.
///
/// Lowercase names, including `data-*` and `aria-*`, are rendered unchanged.
/// CamelCase props are mapped only when React's rendered name is known here;
/// event handlers, `ref`, `children`, uncontrolled defaults, and every other
/// camelCase name are unsupported.
pub(crate) fn html_attribute(name: &str) -> HtmlAttribute {
    match name {
        "key" => HtmlAttribute::Key,
        "style" => HtmlAttribute::Style,
        "dangerouslySetInnerHTML" => HtmlAttribute::InnerHtml,
        "suppressHydrationWarning" | "suppressContentEditableWarning" => HtmlAttribute::Drop,
        "className" => HtmlAttribute::Rename("class"),
        "htmlFor" => HtmlAttribute::Rename("for"),
        "acceptCharset" => HtmlAttribute::Rename("accept-charset"),
        "httpEquiv" => HtmlAttribute::Rename("http-equiv"),
        "accessKey" => HtmlAttribute::Rename("accesskey"),
        "autoCapitalize" => HtmlAttribute::Rename("autocapitalize"),
        "autoComplete" => HtmlAttribute::Rename("autocomplete"),
        "autoFocus" => HtmlAttribute::Rename("autofocus"),
        "autoPlay" => HtmlAttribute::Rename("autoplay"),
        "colSpan" => HtmlAttribute::Rename("colspan"),
        "crossOrigin" => HtmlAttribute::Rename("crossorigin"),
        "dateTime" => HtmlAttribute::Rename("datetime"),
        "encType" => HtmlAttribute::Rename("enctype"),
        "enterKeyHint" => HtmlAttribute::Rename("enterkeyhint"),
        "fetchPriority" => HtmlAttribute::Rename("fetchpriority"),
        "formAction" => HtmlAttribute::Rename("formaction"),
        "formNoValidate" => HtmlAttribute::Rename("formnovalidate"),
        "hrefLang" => HtmlAttribute::Rename("hreflang"),
        "inputMode" => HtmlAttribute::Rename("inputmode"),
        "itemProp" => HtmlAttribute::Rename("itemprop"),
        "itemScope" => HtmlAttribute::Rename("itemscope"),
        "itemType" => HtmlAttribute::Rename("itemtype"),
        "maxLength" => HtmlAttribute::Rename("maxlength"),
        "minLength" => HtmlAttribute::Rename("minlength"),
        "noModule" => HtmlAttribute::Rename("nomodule"),
        "noValidate" => HtmlAttribute::Rename("novalidate"),
        "playsInline" => HtmlAttribute::Rename("playsinline"),
        "readOnly" => HtmlAttribute::Rename("readonly"),
        "referrerPolicy" => HtmlAttribute::Rename("referrerpolicy"),
        "rowSpan" => HtmlAttribute::Rename("rowspan"),
        "srcLang" => HtmlAttribute::Rename("srclang"),
        "srcSet" => HtmlAttribute::Rename("srcset"),
        "tabIndex" => HtmlAttribute::Rename("tabindex"),
        "useMap" => HtmlAttribute::Rename("usemap"),
        name if crate::names::is_lowercase_name(name) && name != "children" && name != "ref" => {
            HtmlAttribute::Same
        }
        _ => HtmlAttribute::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_attribute_escaping_cover_html_and_astro_delimiters() {
        assert_eq!(
            escape_text("a & <b> {c}\u{a0}\t\n\r\"'"),
            "a &amp; &lt;b&gt; &#123;c&#125;&nbsp;&#9;&#10;&#13;\"'"
        );
        assert_eq!(
            escape_attribute("a & \"b\" {c} <d>\n"),
            "a &amp; &quot;b&quot; {c} <d>&#10;"
        );
    }

    #[test]
    fn react_attribute_names_map_to_html_or_fail_closed() {
        assert_eq!(html_attribute("className"), HtmlAttribute::Rename("class"));
        assert_eq!(
            html_attribute("tabIndex"),
            HtmlAttribute::Rename("tabindex")
        );
        assert_eq!(html_attribute("data-id"), HtmlAttribute::Same);
        assert_eq!(html_attribute("aria-label"), HtmlAttribute::Same);
        assert_eq!(html_attribute("href"), HtmlAttribute::Same);
        assert_eq!(html_attribute("key"), HtmlAttribute::Key);
        for unsupported in [
            "onClick",
            "ref",
            "children",
            "defaultValue",
            "strokeWidth",
            "xlinkHref",
            "data_x",
        ] {
            assert_eq!(
                html_attribute(unsupported),
                HtmlAttribute::Unsupported,
                "{unsupported}"
            );
        }
    }
}
