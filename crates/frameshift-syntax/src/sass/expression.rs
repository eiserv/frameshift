//! SassScript precedence, lists, maps, calls, and interpolation.

use std::collections::BTreeSet;

use super::{
    Parser, SassBinaryOperator as Binary, SassError, SassErrorKind as Error,
    SassListSeparator as Separator, SassNodeId as Id, SassNodeKind as Kind,
    SassUnaryOperator as Unary, canonical_name,
};

impl Parser<'_> {
    pub(super) fn expression(&mut self, comma: bool) -> Result<Id, SassError> {
        self.enter()?;
        let first = self.space_list()?;
        let mut items = vec![first];
        if comma {
            while self.eat(",")? {
                if !self.expression_newlines && self.peek().is_some_and(super::newline) {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                self.skip(self.expression_newlines)?;
                if !self.expression_start() {
                    break;
                }
                items.push(self.space_list()?);
            }
        }
        let trailing = self.text[..self.pos].trim_end().ends_with(',');
        let result = if items.len() > 1 || (comma && trailing) {
            self.node(
                self.start(first),
                Kind::List {
                    separator: Separator::Comma,
                    bracketed: false,
                },
                items,
            )
        } else {
            Ok(first)
        };
        self.depth -= 1;
        result
    }

    fn space_list(&mut self) -> Result<Id, SassError> {
        let first = self.binary(0)?;
        let mut items = vec![first];
        loop {
            self.skip(self.expression_newlines)?;
            if !self.expression_start() {
                break;
            }
            // Sass forms space-separated lists even from adjacent primaries.
            items.push(self.binary(0)?);
        }
        if items.len() == 1 {
            return Ok(first);
        }
        self.node(
            self.start(first),
            Kind::List {
                separator: Separator::Space,
                bracketed: false,
            },
            items,
        )
    }

    fn binary(&mut self, minimum: u8) -> Result<Id, SassError> {
        self.enter()?;
        let mut left = self.primary()?;
        loop {
            let before_space = self.end(left);
            self.skip(self.expression_newlines)?;
            let Some((operator, precedence, spelling)) = self.operator() else {
                break;
            };
            if precedence < minimum {
                break;
            }
            // Sass's strict-unary disambiguation: `1 -2` is a space list,
            // whereas `1 - 2` and `1-2` are binary operations.
            if matches!(operator, Binary::Add | Binary::Subtract)
                && self.pos > before_space
                && self.text[self.pos + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| !super::css_space(c))
            {
                break;
            }
            self.expect(spelling)?;
            self.skip(true)?;
            let right = self.binary(precedence + 1)?;
            let end = self.end(right);
            left = self.node(self.start(left), Kind::Binary(operator), vec![left, right])?;
            self.nodes[left.0].span = super::Span::new(self.start(left), end).unwrap();
        }
        self.depth -= 1;
        Ok(left)
    }

    fn operator(&self) -> Option<(Binary, u8, &'static str)> {
        for (text, operator, precedence) in [
            ("or", Binary::Or, 1),
            ("and", Binary::And, 2),
            ("==", Binary::Equal, 3),
            ("!=", Binary::NotEqual, 3),
            ("<=", Binary::LessEqual, 4),
            (">=", Binary::GreaterEqual, 4),
            ("<", Binary::Less, 4),
            (">", Binary::Greater, 4),
            ("+", Binary::Add, 5),
            ("-", Binary::Subtract, 5),
            ("*", Binary::Multiply, 6),
            ("/", Binary::Divide, 6),
            ("%", Binary::Modulo, 6),
            ("=", Binary::SingleEqual, 0),
        ] {
            if operator == Binary::SingleEqual && !self.single_equals {
                continue;
            }
            if !self.at(text) {
                continue;
            }
            if matches!(operator, Binary::Or | Binary::And)
                && self.text[self.pos + text.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| {
                        c.is_ascii_alphanumeric()
                            || c == '_'
                            || c == '-'
                            || c == '\\'
                            || c >= '\u{80}'
                    })
            {
                continue;
            }
            return Some((operator, precedence, text));
        }
        None
    }

    fn important_ahead(&self) -> bool {
        let Some(rest) = self.text[self.pos..].strip_prefix('!') else {
            return false;
        };
        let rest = rest.trim_start_matches(super::css_space);
        rest.get(..9)
            .is_some_and(|s| s.eq_ignore_ascii_case("important"))
            && rest[9..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '-'))
    }

    pub(super) fn expression_start(&self) -> bool {
        if self.depth == self.stop_depth
            && self.stop_words.iter().any(|word| {
                self.at(word)
                    && self.text[self.pos + word.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| {
                            !c.is_ascii_alphanumeric()
                                && !matches!(c, '_' | '-' | '\\')
                                && c < '\u{80}'
                        })
            })
        {
            return false;
        }
        self.important_ahead()
            || self.name_start()
            || self.peek().is_some_and(|c| {
                c.is_ascii_digit()
                    || matches!(
                        c,
                        '$' | '#' | '\'' | '"' | '(' | '[' | '&' | '+' | '-' | '%'
                    )
            })
            || (self.at(".")
                && self.text[self.pos + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit()))
    }

    fn primary(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        if self.at("$") {
            return self.variable();
        }
        if (self.at("u+") || self.at("U+"))
            && self.text[self.pos + 2..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_hexdigit() || c == '?')
        {
            return self.unicode_range();
        }
        if self.peek().is_some_and(|c| matches!(c, '\'' | '"')) {
            return self.quoted();
        }
        if self.at("[") {
            return self.collection(true);
        }
        if self.at("(") {
            return self.collection(false);
        }
        if self.eat("&")? {
            return self.node(start, Kind::ParentSelector, Vec::new());
        }
        if self.keyword("not")? {
            self.skip(true)?;
            let value = self.unary_operand()?;
            return self.node(start, Kind::Unary(Unary::Not), vec![value]);
        }
        if self.at("#{") {
            return self.unquoted();
        }
        if self.peek().is_some_and(|c| c.is_ascii_digit())
            || (self.at(".")
                && self.text[self.pos + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit()))
        {
            return self.number();
        }
        if self.at("+") || (self.at("-") && !self.name_start()) {
            let minus = self.bump()? == Some('-');
            self.skip(true)?;
            let value = self.unary_operand()?;
            return self.node(
                start,
                Kind::Unary(if minus { Unary::Minus } else { Unary::Plus }),
                vec![value],
            );
        }
        if self.eat("#")? {
            let digit_start = self.pos;
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                let mut count = 0;
                while count < 8 && self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.bump()?;
                    count += 1;
                }
                if !matches!(count, 3 | 4 | 6 | 8) {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                return self.node(start, Kind::Color, Vec::new());
            }
            self.name_body()?;
            if self.pos == digit_start {
                return Err(self.error(Error::ExpectedIdentifier));
            }
            let text = &self.text[digit_start..self.pos];
            let color =
                matches!(text.len(), 3 | 4 | 6 | 8) && text.bytes().all(|b| b.is_ascii_hexdigit());
            return self.node(
                start,
                if color {
                    Kind::Color
                } else {
                    Kind::String { quoted: false }
                },
                Vec::new(),
            );
        }
        if self.eat("!")? {
            self.skip(true)?;
            let name = self.identifier()?;
            if !canonical_name(self.spelling(name)).eq_ignore_ascii_case("important") {
                return Err(self.error(Error::ExpectedExpression));
            }
            return self.node(start, Kind::String { quoted: false }, vec![name]);
        }
        if self.eat("%")? {
            return self.node(start, Kind::String { quoted: false }, Vec::new());
        }
        if self.name_start() {
            return self.identifier_expression();
        }
        Err(self.error(Error::ExpectedExpression))
    }

    fn unary_operand(&mut self) -> Result<Id, SassError> {
        self.enter()?;
        let result = self.primary();
        self.depth -= 1;
        result
    }

    pub(super) fn variable(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        self.expect("$")?;
        if !self.name_start() {
            return Err(self.error(Error::ExpectedIdentifier));
        }
        self.name_body()?;
        self.node(start, Kind::Variable, Vec::new())
    }

    fn unicode_range(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        self.bump()?;
        self.expect("+")?;
        let mut count = 0;
        while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            count += 1;
            self.bump()?;
        }
        let mut questions = false;
        while self.eat("?")? {
            count += 1;
            questions = true;
        }
        if count == 0 || count > 6 {
            return Err(self.error(Error::ExpectedSyntax));
        }
        if !questions && self.eat("-")? {
            count = 0;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                count += 1;
                self.bump()?;
            }
            if count == 0 || count > 6 {
                return Err(self.error(Error::ExpectedSyntax));
            }
        }
        if !questions && (self.name_start() || self.at("-") || self.at("#{")) {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.node(start, Kind::UnicodeRange, Vec::new())
    }

    fn number(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump()?;
        }
        if self.at(".")
            && self.text[self.pos + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            self.bump()?;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump()?;
            }
        }
        if self.at("e") || self.at("E") {
            let after_e = &self.text[self.pos + 1..];
            let after_sign = after_e.strip_prefix(['+', '-']).unwrap_or(after_e);
            if after_e.starts_with(['+', '-'])
                && !after_sign.starts_with(|c: char| c.is_ascii_digit())
            {
                return Err(self.error(Error::ExpectedSyntax));
            }
            if after_sign.starts_with(|c: char| c.is_ascii_digit()) {
                self.bump()?;
                if self.at("+") || self.at("-") {
                    self.bump()?;
                }
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.bump()?;
                }
            }
        }
        if !self.eat("%")? && self.name_start() {
            self.name_body()?;
        }
        self.node(start, Kind::Number, Vec::new())
    }

    fn identifier_expression(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        self.name_body()?;
        if self.at("#{") {
            return self.unquoted_from(start);
        }
        let plain = canonical_name(&self.text[start..self.pos]);
        let kind = if plain == "true" || plain == "false" {
            Kind::Boolean
        } else if plain == "null" {
            Kind::Null
        } else {
            Kind::String { quoted: false }
        };
        if !self.at("...") && self.eat(".")? {
            if self.at("$") {
                self.expect("$")?;
                if !self.name_start() {
                    return Err(self.error(Error::ExpectedIdentifier));
                }
                let member = self.pos;
                self.name_body()?;
                if canonical_name(&self.text[member..self.pos]).starts_with('-') {
                    return Err(self.error(Error::InvalidContext));
                }
                return self.node(start, Kind::Variable, Vec::new());
            }
            if !self.name_start() {
                return Err(self.error(Error::ExpectedIdentifier));
            }
            let member = self.pos;
            self.name_body()?;
            if canonical_name(&self.text[member..self.pos]).starts_with('-') {
                return Err(self.error(Error::InvalidContext));
            }
            if !self.at("(") {
                return Err(self.error(Error::ExpectedSyntax));
            }
        }
        if self.at("(") {
            let function_name = self.text[start..self.pos].to_ascii_lowercase();
            let name = self.node(start, Kind::Identifier, Vec::new())?;
            if matches!(
                function_name.as_str(),
                "url" | "-webkit-calc" | "-moz-calc" | "element" | "expression" | "progid"
            ) {
                let contents = self.raw_group()?;
                return self.node(start, Kind::SpecialFunction, vec![name, contents]);
            }
            let args = self.arguments(false, true)?;
            return self.node(start, Kind::Call, vec![name, args]);
        }
        self.node(start, kind, Vec::new())
    }

    fn unquoted(&mut self) -> Result<Id, SassError> {
        self.unquoted_from(self.pos)
    }

    fn unquoted_from(&mut self, start: usize) -> Result<Id, SassError> {
        let mut parts = Vec::new();
        loop {
            if self.at("#{") {
                parts.push(self.interpolation()?);
            } else if self.peek().is_some_and(|c| {
                c.is_ascii_alphanumeric() || c == '_' || c == '-' || c >= '\u{80}' || c == '\\'
            }) {
                self.name_body()?;
            } else {
                break;
            }
        }
        if self.at("(") {
            let name = self.node(start, Kind::InterpolatedText, parts)?;
            let args = self.arguments(false, true)?;
            return self.node(start, Kind::Call, vec![name, args]);
        }
        self.node(start, Kind::String { quoted: false }, parts)
    }

    fn collection(&mut self, bracketed: bool) -> Result<Id, SassError> {
        let start = self.pos;
        self.expect(if bracketed { "[" } else { "(" })?;
        let close = if bracketed { "]" } else { ")" };
        self.skip(true)?;
        if self.eat(close)? {
            return self.node(
                start,
                Kind::List {
                    separator: Separator::Undecided,
                    bracketed,
                },
                Vec::new(),
            );
        }
        let before = self.expression_newlines;
        self.expression_newlines = true;
        let equals = self.single_equals;
        self.single_equals = false;
        let first = self.expression(false)?;
        self.skip(true)?;
        let mut items = vec![first];
        let is_map = !bracketed && self.at(":");
        if is_map {
            self.expect(":")?;
            self.skip(true)?;
            let value = self.expression(false)?;
            items[0] = self.node(self.start(first), Kind::MapEntry, vec![first, value])?;
        }
        let mut comma = false;
        while self.eat(",")? {
            comma = true;
            self.skip(true)?;
            if self.at(close) {
                break;
            }
            let key = self.expression(false)?;
            if is_map {
                self.expect(":")?;
                self.skip(true)?;
                let value = self.expression(false)?;
                items.push(self.node(self.start(key), Kind::MapEntry, vec![key, value])?);
            } else {
                items.push(key);
            }
            self.skip(true)?;
        }
        self.expect(close)?;
        self.expression_newlines = before;
        self.single_equals = equals;
        let kind = if is_map {
            Kind::Map
        } else if !bracketed && !comma {
            Kind::Parenthesized
        } else {
            Kind::List {
                separator: if comma {
                    Separator::Comma
                } else {
                    Separator::Undecided
                },
                bracketed,
            }
        };
        self.node(start, kind, items)
    }

    pub(super) fn arguments(
        &mut self,
        parameters: bool,
        allow_single_equal: bool,
    ) -> Result<Id, SassError> {
        self.enter()?;
        let start = self.pos;
        self.expect("(")?;
        self.skip(true)?;
        let before = self.expression_newlines;
        let equals = self.single_equals;
        self.expression_newlines = true;
        self.single_equals = !parameters && allow_single_equal;
        let mut children = Vec::new();
        let mut names = BTreeSet::new();
        let mut named = false;
        let mut rests = 0;
        while !self.at(")") {
            let arg_start = self.pos;
            let mut parts = Vec::new();
            let first = if parameters {
                self.variable()?
            } else {
                self.expression(false)?
            };
            parts.push(first);
            self.skip(true)?;
            if self.eat(":")? {
                if !matches!(self.nodes[first.0].kind, Kind::Variable)
                    || self.spelling(first).contains('.')
                {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                let name = canonical_name(self.spelling(first)).into_owned();
                if !names.insert(name) {
                    return Err(self.error(Error::DuplicateName));
                }
                named = true;
                self.skip(true)?;
                parts.push(self.expression(false)?);
            } else if parameters {
                let name = canonical_name(self.spelling(first)).into_owned();
                if !names.insert(name) {
                    return Err(self.error(Error::DuplicateName));
                }
            } else if named && !self.at("...") {
                return Err(self.error(Error::ExpectedSyntax));
            }
            if self.eat("...")? {
                rests += 1;
                if parts.len() > 1 || (parameters && rests > 1) || rests > 2 {
                    return Err(self.error(Error::ExpectedSyntax));
                }
            }
            self.skip(true)?;
            children.push(self.node(
                arg_start,
                if parameters {
                    Kind::Parameter
                } else {
                    Kind::Argument
                },
                parts,
            )?);
            if !self.eat(",")? {
                break;
            }
            self.skip(true)?;
            if ((parameters && rests > 0) || rests == 2) && !self.at(")") {
                return Err(self.error(Error::ExpectedSyntax));
            }
        }
        self.expect(")")?;
        self.expression_newlines = before;
        self.single_equals = equals;
        let result = self.node(
            start,
            if parameters {
                Kind::Parameters
            } else {
                Kind::Arguments
            },
            children,
        );
        self.depth -= 1;
        result
    }

    pub(super) fn raw_group(&mut self) -> Result<Id, SassError> {
        self.enter()?;
        let start = self.pos;
        let close = match self.bump()? {
            Some('(') => ")",
            Some('[') => "]",
            Some('{') => "}",
            Some(_) | None => return Err(self.error(Error::ExpectedSyntax)),
        };
        let mut children = Vec::new();
        loop {
            if self.eat(close)? {
                break;
            }
            if self.peek().is_none() {
                return Err(self.error(Error::Unterminated));
            }
            if self.at("#{") {
                children.push(self.interpolation()?);
            } else if self.peek().is_some_and(|c| matches!(c, '\'' | '"')) {
                children.push(self.quoted()?);
            } else if self.peek().is_some_and(|c| matches!(c, '(' | '[' | '{')) {
                children.push(self.raw_group()?);
            } else if self.peek().is_some_and(|c| matches!(c, ')' | ']' | '}')) {
                return Err(self.error(Error::ExpectedSyntax));
            } else if self.at("\\") {
                self.escape(false)?;
            } else {
                self.bump()?;
            }
        }
        let result = self.node(start, Kind::InterpolatedText, children);
        self.depth -= 1;
        result
    }
}
