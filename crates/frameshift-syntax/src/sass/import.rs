//! Import items and URL classification, without loading a stylesheet.

use super::{
    Parser, SassError, SassErrorKind as Error, SassNodeId as Id, SassNodeKind as Kind, SassSyntax,
    css_space, decode_name, module_url, newline,
};

impl Parser<'_> {
    pub(super) fn import_header(
        &mut self,
        children: &mut Vec<Id>,
        disallow_sass: bool,
    ) -> Result<(), SassError> {
        loop {
            self.skip(self.syntax == SassSyntax::Scss)?;
            let start = self.pos;
            // The pinned parser dispatches url() only from literal u/U. A
            // leading escape is an unquoted path in Sass and invalid in SCSS.
            let (url, plain_css, decoded) = if self.syntax == SassSyntax::Indented
                && !self.peek().is_some_and(|c| matches!(c, '\'' | '"'))
                && !self.import_url_ahead()?
            {
                // The indented syntax's legacy unquoted URL is literal text,
                // including spaces and `#{...}`, until comma or line end.
                while self
                    .peek()
                    .is_some_and(|c| c != ',' && c != ';' && !newline(c))
                {
                    self.bump()?;
                }
                let text = &self.text[start..self.pos];
                let css = plain_import(text);
                if !css {
                    self.validate_import_url(text)?;
                }
                (
                    self.node(start, Kind::String { quoted: false }, Vec::new())?,
                    css,
                    None,
                )
            } else if self.peek().is_some_and(|c| matches!(c, 'u' | 'U')) {
                (self.import_url()?, true, None)
            } else {
                // Quoted paths never interpolate, even when classified as CSS.
                let (url, decoded) = self.module_string()?;
                (url, plain_import(&decoded), Some(decoded))
            };
            self.skip(self.syntax == SassSyntax::Scss)?;
            let mut parts = vec![url];
            if self.name_start() || self.at("#{") || self.at("-#{") || self.at("(") {
                parts.push(self.import_modifiers()?);
            }
            let css = plain_css || parts.len() > 1;
            if !css {
                if disallow_sass {
                    return Err(self.error(Error::InvalidContext));
                }
                if let Some(decoded) = decoded {
                    self.validate_import_url(&decoded)?;
                }
            }
            let end = parts.last().map_or(self.end(url), |&id| self.end(id));
            children.push(self.node_between(start, end, Kind::Import { css }, parts)?);
            if !self.eat(",")? {
                break;
            }
        }
        Ok(())
    }

    fn validate_import_url(&mut self, url: &str) -> Result<(), SassError> {
        let work = url
            .len()
            .checked_mul(8)
            .ok_or_else(|| self.error(Error::WorkLimit))?;
        if work > self.limits.max_work.saturating_sub(self.work) {
            return Err(self.error(Error::WorkLimit));
        }
        self.work += work;
        // Dart Sass converts absolute Windows drive paths to file URIs first.
        // UNC paths still require a valid authority in the URI reader.
        let windows = url.as_bytes().get(1) == Some(&b':')
            && url.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && url
                .as_bytes()
                .get(2)
                .is_some_and(|c| matches!(c, b'/' | b'\\'));
        if !windows && module_url::path(url).is_none() {
            return Err(self.error(Error::ExpectedSyntax));
        }
        Ok(())
    }

    fn import_url_ahead(&mut self) -> Result<bool, SassError> {
        if !self.peek().is_some_and(|c| matches!(c, 'u' | 'U')) {
            return Ok(false);
        }
        let start = self.pos;
        let found = self.module_keyword("url", false)? && self.at("(");
        self.pos = start;
        Ok(found)
    }

    fn import_url(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        if !self.module_keyword("url", false)? {
            return Err(self.error(Error::ExpectedSyntax));
        }
        let name = self.node(start, Kind::Identifier, Vec::new())?;
        let contents = self.pos;
        let nodes = self.nodes.len();
        let comments = self.comments.len();
        self.expect("(")?;
        while self.peek().is_some_and(css_space) {
            self.bump()?;
        }
        let mut parts = Vec::new();
        loop {
            if self.eat(")")? {
                let raw = self.node(contents, Kind::InterpolatedText, parts)?;
                return self.node(start, Kind::Call, vec![name, raw]);
            }
            if self.at("#{") {
                parts.push(self.interpolation()?);
            } else if self.at("\\") {
                self.escape(false)?;
            } else if self.peek().is_some_and(css_space) {
                while self.peek().is_some_and(css_space) {
                    self.bump()?;
                }
                if !self.at(")") {
                    break;
                }
            } else if self.peek().is_some_and(|c| {
                matches!(c, '!' | '%' | '&' | '#') || ('*'..='~').contains(&c) || c >= '\u{80}'
            }) {
                self.bump()?;
            } else {
                break;
            }
        }
        // Raw URL parsing and SassScript share a prefix. Retry only at that
        // known context, retaining work charges and discarding speculative
        // nodes and comments. Grammar and limit failures are never recovered.
        self.pos = contents;
        self.nodes.truncate(nodes);
        self.comments.truncate(comments);
        let arguments = self.arguments(false, true)?;
        self.node(start, Kind::Call, vec![name, arguments])
    }

    fn import_modifiers(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        // Modifier grammar is a separate remaining increment. Preserve its
        // comma ambiguity as Sass does: after a bare identifier, a comma
        // starts a media-query list rather than another import item.
        loop {
            if self.at("#{") || self.name_start() || self.at("-#{") {
                let name_start = self.pos;
                if self.at("-#{") {
                    self.bump()?;
                }
                let name = self.interpolated_name()?;
                self.nodes[name.0].span = super::Span::new(name_start, self.end(name)).unwrap();
                parts.push(name);
                if self.at("(")
                    && !decode_name(self.spelling(name), false).eq_ignore_ascii_case("and")
                {
                    parts.push(self.raw_group()?);
                    self.skip(self.syntax == SassSyntax::Scss)?;
                } else {
                    self.skip(self.syntax == SassSyntax::Scss)?;
                    if self.eat(",")? {
                        let (rest, _) = self.raw_header(false)?;
                        if self.start(rest) == self.end(rest) {
                            return Err(self.error(Error::ExpectedSyntax));
                        }
                        parts.push(rest);
                        break;
                    }
                }
            } else if self.at("(") {
                let (rest, _) = self.raw_header(false)?;
                parts.push(rest);
                break;
            } else {
                break;
            }
        }
        let end = self.end(*parts.last().unwrap());
        self.node_between(start, end, Kind::ImportModifiers, parts)
    }
}

fn plain_import(url: &str) -> bool {
    url.encode_utf16().take(5).count() >= 5
        && (url.ends_with(".css")
            || url.starts_with("//")
            || url.starts_with("http://")
            || url.starts_with("https://"))
}
