//! Import items and URL classification, without loading a stylesheet.

use super::{
    Parser, SassConditionOperator as Operator, SassError, SassErrorKind as Error, SassNodeId as Id,
    SassNodeKind as Kind, SassSyntax, condition::ValueOptions, module_url, newline,
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
        if let Some(parts) = self.raw_url_contents()? {
            let raw = self.node(contents, Kind::InterpolatedText, parts)?;
            return self.node(start, Kind::Call, vec![name, raw]);
        }
        // Raw URL parsing and SassScript share a prefix. Retry only at that
        // known context, retaining work charges and discarding speculative
        // nodes and comments. Grammar and limit failures are never recovered.
        let arguments = self.arguments(false, true)?;
        self.node(start, Kind::Call, vec![name, arguments])
    }

    fn import_modifiers(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        // As in Dart Sass, a comma after a bare name starts a media query
        // list rather than another import item, while a comma after a
        // modifier function can introduce another import item.
        loop {
            if self.at("#{") || self.name_start() || self.at("-#{") {
                let name = self.condition_identifier()?;
                let plain = self.plain_name(name).map(|name| name.to_lowercase());
                if plain.as_deref() != Some("and") && self.eat("(")? {
                    let argument = if plain.as_deref() == Some("supports") {
                        self.import_supports()?
                    } else {
                        self.declaration_value(ValueOptions {
                            allow_empty: true,
                            allow_semicolon: true,
                            allow_colon: true,
                            allow_open_brace: true,
                            silent_comments: true,
                            lines: true,
                        })?
                    };
                    self.expect(")")?;
                    let function_start = self.start(name);
                    parts.push(self.node(
                        function_start,
                        Kind::ImportFunction,
                        vec![name, argument],
                    )?);
                    self.skip(self.syntax == SassSyntax::Scss)?;
                } else {
                    parts.push(name);
                    self.skip(self.syntax == SassSyntax::Scss)?;
                    if self.eat(",")? {
                        parts.push(self.media_query_list()?);
                        break;
                    }
                }
            } else if self.at("(") {
                parts.push(self.media_query_list()?);
                break;
            } else {
                break;
            }
        }
        let end = self.end(*parts.last().unwrap());
        self.node_between(start, end, Kind::ImportModifiers, parts)
    }

    /// The condition inside `supports(...)`, which also admits a bare
    /// declaration or function.
    fn import_supports(&mut self) -> Result<Id, SassError> {
        self.skip(true)?;
        let start = self.pos;
        if self.name_start() && self.module_keyword("not", false)? {
            let not = self.node(start, Kind::ConditionOperator(Operator::Not), Vec::new())?;
            self.skip(true)?;
            let operand = self.supports_in_parens()?;
            return self.node(start, Kind::SupportsNegation, vec![not, operand]);
        }
        if self.at("(") {
            return self.supports_condition(true);
        }
        if self.at("#{") || self.name_start() || self.at("-#{") {
            let nodes = self.nodes.len();
            let comments = self.comments.len();
            let name = self.condition_identifier()?;
            if self.eat("(")? {
                let arguments = self.declaration_value(ValueOptions {
                    allow_empty: true,
                    allow_semicolon: true,
                    allow_colon: true,
                    allow_open_brace: true,
                    silent_comments: true,
                    lines: true,
                })?;
                self.expect(")")?;
                return self.node(start, Kind::SupportsFunction, vec![name, arguments]);
            }
            // Not a function: reparse the name as a declaration expression.
            self.pos = start;
            self.nodes.truncate(nodes);
            self.comments.truncate(comments);
        }
        let before = self.expression_newlines;
        self.expression_newlines = true;
        let name = self.expression(true)?;
        self.expression_newlines = before;
        self.expect(":")?;
        let value = self.supports_declaration_value(name)?;
        self.node(start, Kind::SupportsDeclaration, vec![name, value])
    }
}

fn plain_import(url: &str) -> bool {
    url.encode_utf16().take(5).count() >= 5
        && (url.ends_with(".css")
            || url.starts_with("//")
            || url.starts_with("http://")
            || url.starts_with("https://"))
}
