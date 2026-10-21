//! Module rules are parsed without interpolation, evaluation, or loading.

use super::{
    Parser, SassAtRule as At, SassError, SassErrorKind as Error, SassNodeId as Id,
    SassNodeKind as Kind, SassSyntax, canonical_name, decode_name, module_url, newline,
};

impl Parser<'_> {
    pub(super) fn module_header(
        &mut self,
        kind: At,
        children: &mut Vec<Id>,
    ) -> Result<(), SassError> {
        self.skip(true)?;
        let (url, decoded) = self.module_string()?;
        // URI validation, normalization, and namespace decoding make a bounded
        // number of linear passes. Charge them before allocating their buffers.
        let work = decoded
            .len()
            .checked_mul(8)
            .ok_or_else(|| self.error(Error::WorkLimit))?;
        if work > self.limits.max_work.saturating_sub(self.work) {
            return Err(self.error(Error::WorkLimit));
        }
        self.work += work;
        let path = module_url::path(&decoded).ok_or_else(|| self.error(Error::ExpectedSyntax))?;
        children.push(url);
        self.skip(self.syntax == SassSyntax::Scss)?;
        let clause = self.pos;
        if self.module_keyword("as", false)? {
            self.skip(true)?;
            let mut names = Vec::new();
            let global = kind == At::Use && self.eat("*")?;
            if !global {
                names.push(self.identifier()?);
                if kind == At::Forward {
                    self.expect("*")?;
                }
            }
            children.push(self.node(
                clause,
                if kind == At::Use {
                    Kind::ModuleNamespace { global }
                } else {
                    Kind::ForwardPrefix
                },
                names,
            )?);
            self.skip(self.syntax == SassSyntax::Scss)?;
        } else if kind == At::Use && !module_url::valid_namespace(&path) {
            return Err(self.error(Error::ExpectedIdentifier));
        }
        let clause = self.pos;
        if kind == At::Forward {
            let shown = self.module_keyword("show", false)?;
            if shown || self.module_keyword("hide", false)? {
                let mut members = Vec::new();
                loop {
                    self.skip(true)?;
                    members.push(if self.at("$") {
                        self.variable()?
                    } else {
                        self.identifier()?
                    });
                    self.skip(self.syntax == SassSyntax::Scss)?;
                    if !self.eat(",")? {
                        break;
                    }
                }
                children.push(self.node_between(
                    clause,
                    self.end(*members.last().unwrap()),
                    Kind::ForwardVisibility { hidden: !shown },
                    members,
                )?);
            }
        }
        if self.module_keyword("with", false)? {
            self.skip(true)?;
            children.push(self.configuration(kind == At::Forward)?);
        }
        Ok(())
    }

    pub(super) fn module_string(&mut self) -> Result<(Id, String), SassError> {
        let start = self.pos;
        if !self.peek().is_some_and(|c| matches!(c, '\'' | '"')) {
            return Err(self.error(Error::ExpectedSyntax));
        }
        let quote = self.bump()?.unwrap();
        let mut decoded = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.error(Error::Unterminated));
            };
            if c == quote {
                self.bump()?;
                break;
            }
            if newline(c) {
                return Err(self.error(Error::Unterminated));
            }
            if c == '\\' {
                let escape = self.pos;
                if self.text[self.pos + 1..]
                    .chars()
                    .next()
                    .is_some_and(newline)
                {
                    self.bump()?;
                    self.bump()?;
                } else {
                    self.escape(false)?;
                    // The pinned plain-string reader consumes one escaped
                    // whitespace scalar, including only CR in a CRLF pair.
                    if self.text[escape..self.pos].ends_with("\r\n") {
                        self.pos -= 1;
                    }
                    decoded.push_str(&decode_name(&self.text[escape..self.pos], false));
                }
            } else {
                decoded.push(self.bump()?.unwrap());
            }
        }
        Ok((
            self.node(start, Kind::String { quoted: true }, Vec::new())?,
            decoded,
        ))
    }

    fn configuration(&mut self, guarded: bool) -> Result<Id, SassError> {
        let start = self.pos;
        self.expect("(")?;
        self.skip(true)?;
        let before = self.expression_newlines;
        self.expression_newlines = true;
        let mut children = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        while !self.at(")") {
            let start = self.pos;
            let variable = self.variable()?;
            if !names.insert(canonical_name(self.spelling(variable)).into_owned()) {
                return Err(self.error(Error::DuplicateName));
            }
            self.skip(true)?;
            self.expect(":")?;
            self.skip(true)?;
            let value = self.expression(false)?;
            let mut parts = vec![variable, value];
            self.skip(true)?;
            if guarded && self.at("!") {
                let flag = self.pos;
                self.expect("!")?;
                if !self.module_keyword("default", true)? {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                parts.push(self.node(flag, Kind::Flag, Vec::new())?);
                self.skip(true)?;
            }
            children.push(self.node(start, Kind::Argument, parts)?);
            if !self.eat(",")? {
                break;
            }
            self.skip(true)?;
        }
        if children.is_empty() {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.expect(")")?;
        self.expression_newlines = before;
        self.node(start, Kind::Arguments, children)
    }

    pub(super) fn module_keyword(
        &mut self,
        word: &str,
        sensitive: bool,
    ) -> Result<bool, SassError> {
        let start = self.pos;
        for expected in word.chars() {
            let actual = if self.at("\\") {
                let escape = self.pos;
                self.escape(false)?;
                decode_name(&self.text[escape..self.pos], false)
                    .chars()
                    .next()
            } else {
                self.bump()?
            };
            if !actual
                .is_some_and(|c| c == expected || (!sensitive && c.eq_ignore_ascii_case(&expected)))
            {
                self.pos = start;
                return Ok(false);
            }
        }
        if self.peek().is_some_and(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '\\') || c >= '\u{80}'
        }) {
            self.pos = start;
            return Ok(false);
        }
        Ok(true)
    }
}
