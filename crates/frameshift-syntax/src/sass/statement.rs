//! Stylesheet statements and the two surface block grammars.

use super::{
    Parser, SassAtRule as At, SassError, SassErrorKind as Error, SassNodeId as Id,
    SassNodeKind as Kind, SassSyntax, canonical_name,
    condition::ValueOptions,
    css_space, decode_name, newline,
    selector::{SelectorContext, plain_identifier, unvendor},
};

#[derive(Clone, Copy)]
pub(super) struct Context {
    root: bool,
    style: bool,
    function: bool,
    mixin: bool,
    content: bool,
    control: bool,
    declaration: bool,
    plain_css_function: bool,
    selectors: SelectorContext,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            root: true,
            style: false,
            function: false,
            mixin: false,
            content: false,
            control: false,
            declaration: false,
            plain_css_function: false,
            selectors: SelectorContext::Normal,
        }
    }
}

impl Parser<'_> {
    pub(super) fn statements(
        &mut self,
        indent: usize,
        context: Context,
    ) -> Result<Vec<Id>, SassError> {
        self.enter()?;
        let mut items = Vec::new();
        let mut use_allowed = true;
        let mut allow_else = false;
        loop {
            self.skip_statements()?;
            if self.peek().is_none() {
                break;
            }
            if self.syntax == SassSyntax::Scss && self.at("}") {
                if context.root {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                break;
            }
            if self.syntax == SassSyntax::Indented {
                let actual = self.current_indent()?;
                if actual < indent {
                    break;
                }
                if actual != indent {
                    return Err(self.error(Error::InvalidIndentation));
                }
            }
            if self.syntax == SassSyntax::Scss && self.eat(";")? {
                continue;
            }
            // Sass discards root charset rules before ordinary statement
            // dispatch. Retain their source, without closing the module prefix.
            // Dart Sass 1.105.2 matches this literal prefix, even before name
            // characters; a boundary check would change reference acceptance.
            if context.root && self.at("@charset") {
                let start = self.pos;
                self.expect("@")?;
                let name_start = self.pos;
                self.expect("charset")?;
                let name = self.node(name_start, Kind::Identifier, Vec::new())?;
                self.skip(self.syntax == SassSyntax::Scss)?;
                let (value, _) = self.module_string()?;
                self.separator()?;
                items.push(self.node(start, Kind::AtRule(At::Css), vec![name, value])?);
                continue;
            }
            let forced_selector =
                self.syntax == SassSyntax::Indented && !context.declaration && self.at("\\");
            let statement =
                if self.at("$") || (!forced_selector && self.namespaced_variable_ahead()?) {
                    self.variable_statement()?
                } else if self.at("@")
                    || (self.syntax == SassSyntax::Indented
                        && !context.function
                        && !context.declaration
                        && (self.at("=") || (self.at("+") && self.name_start_at(self.pos + 1))))
                {
                    self.at_rule(indent, context, use_allowed, allow_else)?
                } else {
                    if context.function {
                        return Err(self.error(Error::InvalidContext));
                    }
                    self.style_or_declaration(indent, context)?
                };
            allow_else = matches!(
                self.nodes[statement.0].kind,
                Kind::AtRule(At::If | At::ElseIf)
            );
            if !matches!(
                self.nodes[statement.0].kind,
                Kind::VariableDeclaration | Kind::AtRule(At::Use | At::Forward)
            ) {
                use_allowed = false;
            }
            items.push(statement);
        }
        self.depth -= 1;
        Ok(items)
    }

    fn current_indent(&mut self) -> Result<usize, SassError> {
        let start = self.text[..self.pos]
            .rfind(['\r', '\n', '\u{c}'])
            .map_or(0, |i| i + 1);
        let prefix = &self.text[start..self.pos];
        if !prefix.chars().all(|c| matches!(c, ' ' | '\t' | '\u{feff}')) {
            return Err(self.error(Error::InvalidIndentation));
        }
        let mut count = 0;
        for c in prefix.chars().filter(|&c| c != '\u{feff}') {
            self.visit()?;
            if let Some(previous) = self.indentation {
                if previous != c {
                    return Err(self.error(Error::InvalidIndentation));
                }
            } else {
                self.indentation = Some(c);
            }
            count += 1;
        }
        Ok(count)
    }

    fn body(
        &mut self,
        indent: usize,
        context: Context,
        required: bool,
    ) -> Result<Vec<Id>, SassError> {
        self.skip(self.syntax == SassSyntax::Scss)?;
        if self.syntax == SassSyntax::Scss {
            if !self.eat("{")? {
                return if required {
                    Err(self.error(Error::ExpectedSyntax))
                } else {
                    Ok(Vec::new())
                };
            }
            let items = self.statements(
                0,
                Context {
                    root: false,
                    ..context
                },
            )?;
            self.expect("}")?;
            Ok(items)
        } else {
            if !self.peek().is_none_or(newline) {
                return Err(self.error(Error::ExpectedSyntax));
            }
            self.skip_statements()?;
            if self.peek().is_none() {
                return Ok(Vec::new());
            }
            let next = self.current_indent()?;
            if next <= indent {
                return Ok(Vec::new());
            }
            self.statements(
                next,
                Context {
                    root: false,
                    ..context
                },
            )
        }
    }

    fn separator(&mut self) -> Result<(), SassError> {
        self.skip(self.syntax == SassSyntax::Scss)?;
        if self.syntax == SassSyntax::Scss {
            if self.eat(";")? || self.at("}") || self.peek().is_none() {
                return Ok(());
            }
        } else {
            self.eat(";")?;
            self.skip(false)?;
            if self.peek().is_none_or(newline) {
                return Ok(());
            }
        }
        Err(self.error(Error::ExpectedSyntax))
    }

    fn looking_at_body(&mut self, indent: usize) -> Result<bool, SassError> {
        if self.syntax == SassSyntax::Scss {
            return Ok(self.at("{"));
        }
        if !self.peek().is_none_or(newline) {
            return Ok(false);
        }
        let checkpoint = self.checkpoint();
        let result = (|| {
            self.skip_statements()?;
            Ok(self.peek().is_some() && self.current_indent()? > indent)
        })();
        self.restore(&checkpoint);
        result
    }

    fn namespaced_variable_ahead(&mut self) -> Result<bool, SassError> {
        if !self.name_start() {
            return Ok(false);
        }
        let pos = self.pos;
        self.name_body()?;
        let result = self.at(".$");
        self.pos = pos;
        Ok(result)
    }

    fn variable_statement(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let namespaced = !self.at("$");
        if namespaced {
            self.name_body()?;
            self.expect(".$")?;
            if !self.name_start() {
                return Err(self.error(Error::ExpectedIdentifier));
            }
            self.name_body()?;
        }
        let variable = if namespaced {
            self.node(start, Kind::Variable, Vec::new())?
        } else {
            self.variable()?
        };
        self.skip(true)?;
        self.expect(":")?;
        self.skip(true)?;
        let value = self.expression(true)?;
        let mut children = vec![variable, value];
        loop {
            self.skip(self.syntax == SassSyntax::Scss)?;
            if !self.at("!") {
                break;
            }
            let start = self.pos;
            self.bump()?;
            let name = self.identifier()?;
            let decoded = canonical_name(self.spelling(name));
            if decoded != "default" && (decoded != "global" || namespaced) {
                return Err(self.error(Error::ExpectedSyntax));
            }
            children.push(self.node(start, Kind::Flag, vec![name])?);
        }
        self.separator()?;
        self.node(start, Kind::VariableDeclaration, children)
    }

    fn at_rule(
        &mut self,
        indent: usize,
        context: Context,
        use_allowed: bool,
        allow_else: bool,
    ) -> Result<Id, SassError> {
        let start = self.pos;
        let mut children = Vec::new();
        let mut kind = if self.eat("=")? {
            self.skip(true)?;
            At::Mixin
        } else if self.eat("+")? {
            At::Include
        } else {
            self.expect("@")?;
            let name = self.interpolated_name()?;
            let decoded = decode_name(self.spelling(name), false);
            let kind = match decoded.as_ref() {
                "use" => At::Use,
                "forward" => At::Forward,
                "import" => At::Import,
                "mixin" => At::Mixin,
                "function" => At::Function,
                "include" => At::Include,
                "content" => At::Content,
                "return" => At::Return,
                "if" => At::If,
                "else" => At::Else,
                "elseif" => At::ElseIf,
                "each" => At::Each,
                "for" => At::For,
                "while" => At::While,
                "extend" => At::Extend,
                "at-root" => At::AtRoot,
                "debug" => At::Debug,
                "warn" => At::Warn,
                "error" => At::Error,
                "media" => At::Media,
                "supports" => At::Supports,
                _ => At::Css,
            };
            children.push(name);
            kind
        };
        if kind == At::Function {
            // The reference dispatches on the literal prefix before decoding
            // a Sass callable name or applying callable context restrictions.
            self.skip(true)?;
            if self.at("--") {
                kind = At::Css;
            }
        }
        if (matches!(kind, At::Use | At::Forward) && (!context.root || !use_allowed))
            || (kind == At::Return && !context.function)
            || (kind == At::Content && !context.mixin)
            || (kind == At::Extend && !context.style && !context.mixin && !context.content)
            || (matches!(kind, At::Else | At::ElseIf) && !allow_else)
            || (context.declaration
                && matches!(
                    kind,
                    At::Css
                        | At::Media
                        | At::Supports
                        | At::Import
                        | At::Extend
                        | At::AtRoot
                        | At::Function
                        | At::Mixin
                        | At::Use
                        | At::Forward
                ))
            || (matches!(kind, At::Function | At::Mixin)
                && (context.function || context.mixin || context.content || context.control))
            || (context.function
                && matches!(
                    kind,
                    At::Css
                        | At::Media
                        | At::Supports
                        | At::Import
                        | At::Include
                        | At::Extend
                        | At::AtRoot
                ))
        {
            return Err(self.error(Error::InvalidContext));
        }
        self.skip(
            self.syntax == SassSyntax::Scss
                || matches!(
                    kind,
                    At::Mixin
                        | At::Function
                        | At::Include
                        | At::Return
                        | At::Debug
                        | At::Warn
                        | At::Error
                        | At::If
                        | At::Each
                        | At::For
                        | At::While
                ),
        )?;
        if kind == At::Else && self.keyword("if")? {
            kind = At::ElseIf;
            self.skip(true)?;
        }
        let mut block = false;
        let mut optional_block = false;
        let mut child_context = context;
        match kind {
            At::Use | At::Forward => {
                self.module_header(kind, &mut children)?;
            }
            At::Mixin | At::Function => {
                let callable = self.identifier()?;
                let plain = decode_name(self.spelling(callable), false);
                if kind == At::Mixin && self.spelling(callable).starts_with("--") {
                    return Err(self.error(Error::InvalidContext));
                }
                let unprefixed = plain
                    .strip_prefix('-')
                    .and_then(|s| s.split_once('-').map(|(_, end)| end))
                    .unwrap_or(&plain);
                if kind == At::Function
                    && (matches!(plain.as_ref(), "and" | "or" | "not" | "url" | "expression")
                        || unprefixed == "element"
                        || plain.eq_ignore_ascii_case("type"))
                {
                    return Err(self.error(Error::InvalidContext));
                }
                children.push(callable);
                self.skip(kind == At::Function || self.syntax == SassSyntax::Scss)?;
                if self.at("(") {
                    children.push(self.arguments(true, false, false)?);
                } else if kind == At::Function {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                child_context.function = kind == At::Function;
                child_context.mixin = kind == At::Mixin;
                // An include site decides whether selectors are keyframes.
                child_context.selectors = SelectorContext::Unknown;
                block = true;
            }
            At::Include => {
                child_context.content = true;
                child_context.selectors = SelectorContext::Unknown;
                children.push(self.identifier()?);
                if self.eat(".")? {
                    let member = self.identifier()?;
                    if canonical_name(self.spelling(member)).starts_with('-') {
                        return Err(self.error(Error::InvalidContext));
                    }
                    children.push(member);
                }
                self.skip(self.syntax == SassSyntax::Scss)?;
                if self.at("(") {
                    children.push(self.arguments(false, false, false)?);
                }
                self.skip(self.syntax == SassSyntax::Scss)?;
                if self.scan_identifier_text("using")? {
                    self.skip(true)?;
                    children.push(self.arguments(true, false, false)?);
                    block = true;
                } else {
                    optional_block = true;
                }
            }
            At::Content => {
                if self.at("(") {
                    children.push(self.arguments(false, false, false)?);
                }
            }
            At::Return | At::Debug | At::Warn | At::Error => {
                children.push(self.expression(true)?);
            }
            At::If | At::While => {
                children.push(self.expression(true)?);
                child_context.control = true;
                block = true;
            }
            At::Else | At::ElseIf => {
                if kind == At::ElseIf {
                    children.push(self.expression(true)?);
                }
                child_context.control = true;
                block = true;
            }
            At::Each => {
                children.push(self.variable()?);
                self.skip(true)?;
                while self.eat(",")? {
                    self.skip(true)?;
                    children.push(self.variable()?);
                    self.skip(true)?;
                }
                if !self.keyword("in")? {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                self.skip(true)?;
                children.push(self.expression(true)?);
                child_context.control = true;
                block = true;
            }
            At::For => {
                children.push(self.variable()?);
                self.skip(true)?;
                if !self.keyword("from")? {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                self.skip(true)?;
                children.push(self.for_bound()?);
                self.skip(true)?;
                if !self.keyword("to")? && !self.keyword("through")? {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                self.skip(true)?;
                children.push(self.expression(true)?);
                child_context.control = true;
                block = true;
            }
            At::AtRoot => {
                if self.at("(") {
                    children.push(self.at_root_query()?);
                    // A query can leave the enclosing keyframes rule.
                    if context.selectors == SelectorContext::Keyframes {
                        child_context.selectors = SelectorContext::Unknown;
                    }
                } else if !self.at("{") && !self.peek().is_none_or(newline) {
                    // An inline selector is one style rule with its own body.
                    let rule_start = self.pos;
                    let nodes = self.nodes.len();
                    let comments = self.comments.len();
                    let header = self.raw_header()?;
                    if self.start(header) == self.end(header) {
                        return Err(self.error(Error::ExpectedSyntax));
                    }
                    children.push(
                        self.style_rule(rule_start, header, nodes, comments, indent, context)?,
                    );
                    return self.node(start, Kind::AtRule(kind), children);
                }
                block = true;
            }
            At::Extend => {
                self.extend_header(&mut children)?;
            }
            At::Import => {
                self.import_header(&mut children, context.mixin || context.control)?;
            }
            At::Media => {
                let query = self.media_query_list()?;
                // Evaluation parses value-dependent query text again.
                if self.media_depends_on_values(query)? {
                    self.media_evidence.push(query);
                }
                children.push(query);
                block = true;
            }
            At::Supports => {
                children.push(self.supports_condition(false)?);
                block = true;
            }
            At::Css => {
                if self.at("!") {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                let name = children[0];
                let plain_function = self
                    .plain_name(name)
                    .is_some_and(|name| name.eq_ignore_ascii_case("function"));
                if self.plain_name(name).as_deref() == Some("-moz-document") {
                    children.push(self.moz_document_value()?);
                    block = true;
                } else if plain_function {
                    children.push(self.declaration_value(ValueOptions {
                        allow_empty: true,
                        allow_semicolon: false,
                        allow_colon: true,
                        allow_open_brace: false,
                        silent_comments: true,
                        lines: false,
                    })?);
                    child_context.plain_css_function = true;
                } else {
                    children.push(self.declaration_value(ValueOptions {
                        allow_empty: true,
                        allow_semicolon: false,
                        allow_colon: true,
                        allow_open_brace: false,
                        silent_comments: true,
                        lines: false,
                    })?);
                }
                optional_block = true;
                child_context.style = true;
                // Evaluation parses keyframe selectors when the evaluated name
                // without a vendor prefix is exactly `keyframes`.
                child_context.selectors = if self.nodes[name.0].kind != Kind::Identifier {
                    SelectorContext::Unknown
                } else if context.selectors != SelectorContext::Unknown
                    && unvendor(&plain_identifier(self.spelling(name))) == "keyframes"
                {
                    SelectorContext::Keyframes
                } else {
                    context.selectors
                };
            }
        }
        self.skip(self.syntax == SassSyntax::Scss)?;
        if block || (optional_block && self.at("{")) {
            children.extend(self.body(indent, child_context, true)?);
        } else if optional_block && self.syntax == SassSyntax::Indented {
            // With indentation, an absent optional body leaves the next
            // sibling at its original position after layout whitespace.
            let end = self.pos;
            self.separator()?;
            let body = self.body(indent, child_context, false)?;
            if body.is_empty() {
                let after = self.pos;
                let id = self.node(start, Kind::AtRule(kind), children)?;
                self.nodes[id.0].span = super::Span::new(start, end).unwrap();
                self.pos = after;
                return Ok(id);
            }
            children.extend(body);
        } else {
            self.separator()?;
        }
        self.node(start, Kind::AtRule(kind), children)
    }

    fn for_bound(&mut self) -> Result<Id, SassError> {
        // The grammar's `to`/`through` delimiter is recognized only at the
        // outermost expression list, never inside calls or parentheses.
        let before = self.stop_words;
        let depth = self.stop_depth;
        self.stop_words = &["to", "through"];
        self.stop_depth = self.depth + 1;
        let result = self.expression(true);
        self.stop_words = before;
        self.stop_depth = depth;
        result
    }

    pub(super) fn interpolated_name(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        if self.at("-#{") {
            self.bump()?;
        }
        if self.name_start() {
            self.name_body()?;
        } else if !self.at("#{") {
            return Err(self.error(Error::ExpectedIdentifier));
        }
        while self.at("#{") {
            parts.push(self.interpolation()?);
            self.name_body()?;
        }
        self.node(
            start,
            if parts.is_empty() {
                Kind::Identifier
            } else {
                Kind::InterpolatedText
            },
            parts,
        )
    }

    fn style_or_declaration(&mut self, indent: usize, context: Context) -> Result<Id, SassError> {
        let start = self.pos;
        let force_style =
            self.syntax == SassSyntax::Indented && !context.declaration && self.eat("\\")?;
        let allows = context.style || context.mixin || context.content || context.declaration;
        if allows && !force_style {
            let checkpoint = self.checkpoint();
            if let Some(declaration) = self.raw_declaration(start, context)? {
                return Ok(declaration);
            }
            if let Some(declaration) = self.property_declaration(indent, context)? {
                return Ok(declaration);
            }
            self.restore(&checkpoint);
        }
        if context.declaration {
            return Err(self.error(Error::ExpectedSyntax));
        }
        let nodes = self.nodes.len();
        let comments = self.comments.len();
        let header = self.raw_header()?;
        if self.start(header) == self.end(header) {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.style_rule(start, header, nodes, comments, indent, context)
    }

    fn property_declaration(
        &mut self,
        indent: usize,
        context: Context,
    ) -> Result<Option<Id>, SassError> {
        let start = self.pos;
        if self
            .peek()
            .is_some_and(|c| matches!(c, '*' | ':' | '.' | '#'))
            && !self.at("#{")
        {
            self.bump()?;
            self.skip(self.syntax == SassSyntax::Scss)?;
        }
        if !self.name_start() && !self.at("#{") && !self.at("-#{") {
            return Ok(None);
        }
        let name = self.interpolated_name()?;
        if self.at("/*") {
            self.raw_loud_comment()?;
            self.nodes[name.0].span = super::Span::new(self.start(name), self.pos).unwrap();
            self.nodes[name.0].kind = Kind::InterpolatedText;
        }
        self.skip(self.syntax == SassSyntax::Scss)?;
        if !self.eat(":")? || (!context.declaration && self.at(":")) {
            return Ok(None);
        }
        if self.syntax == SassSyntax::Indented
            && !context.declaration
            && (self.name_start() || self.at("#{") || self.at("-#{"))
        {
            return Ok(None);
        }
        let whitespace = self.skip(self.syntax == SassSyntax::Scss)?;
        let ambiguous = !context.declaration
            && !whitespace
            && (self.name_start() || self.at("#{") || self.at("-#{"));
        let before_value = self.checkpoint();
        let mut children = vec![name];
        let result = (|| {
            if !self.at("{") && !self.peek().is_none_or(newline) && !self.at(";") && !self.at("}") {
                children.push(self.expression(true)?);
            }
            self.skip(self.syntax == SassSyntax::Scss)?;
            let block = self.at("{");
            // Decide before reading children, so nested ambiguous selectors
            // cannot repeatedly speculate over the same descendant bodies.
            if ambiguous && self.looking_at_body(indent)? {
                return Err(self.error(Error::ExpectedSyntax));
            }
            let body = if block
                || (self.syntax == SassSyntax::Indented && self.peek().is_none_or(newline))
            {
                self.body(
                    indent,
                    Context {
                        declaration: true,
                        ..context
                    },
                    block,
                )?
            } else {
                self.separator()?;
                Vec::new()
            };
            if children.len() == 1 && body.is_empty() && !block {
                return Err(self.error(Error::ExpectedExpression));
            }
            children.extend(body);
            self.node(start, Kind::Declaration, children).map(Some)
        })();
        match result {
            Err(error)
                if ambiguous
                    && !matches!(
                        error.kind,
                        Error::NodeLimit | Error::DepthLimit | Error::WorkLimit
                    ) =>
            {
                self.restore(&before_value);
                self.almost_any_value()?;
                if self.syntax == SassSyntax::Scss && self.at(";") {
                    Err(error)
                } else {
                    Ok(None)
                }
            }
            other => other,
        }
    }

    fn raw_declaration(&mut self, start: usize, context: Context) -> Result<Option<Id>, SassError> {
        let checkpoint = self.checkpoint();
        let name = match self.interpolated_name() {
            Ok(name) => name,
            Err(error)
                if matches!(
                    error.kind,
                    Error::NodeLimit | Error::DepthLimit | Error::WorkLimit
                ) =>
            {
                return Err(error);
            }
            Err(_) => {
                self.restore(&checkpoint);
                return Ok(None);
            }
        };
        // A glued loud comment is part of the reference's property name.
        if self.at("/*") {
            self.raw_loud_comment()?;
            self.nodes[name.0].span = super::Span::new(self.start(name), self.pos).unwrap();
            self.nodes[name.0].kind = Kind::InterpolatedText;
        }
        self.skip(self.syntax == SassSyntax::Scss)?;
        let custom = self.spelling(name).starts_with("--");
        if !(custom
            || (context.plain_css_function
                && self
                    .plain_name(name)
                    .is_some_and(|name| name.eq_ignore_ascii_case("result"))))
            || !self.eat(":")?
        {
            self.restore(&checkpoint);
            return Ok(None);
        }
        if custom && context.declaration {
            return Err(self.error(Error::InvalidContext));
        }
        // Result declarations are raw CSS, even when their contents are not
        // SassScript or look like a nested property block. Preserve // text.
        let end = self.peek().is_none_or(|c| match self.syntax {
            SassSyntax::Scss => matches!(c, ';' | '}' | '{'),
            SassSyntax::Indented => newline(c),
        });
        let value = if end {
            self.node(self.pos, Kind::InterpolatedText, Vec::new())?
        } else {
            let value = self.declaration_value(ValueOptions {
                allow_empty: false,
                allow_semicolon: false,
                allow_colon: true,
                allow_open_brace: true,
                silent_comments: false,
                lines: false,
            })?;
            if self.syntax == SassSyntax::Indented {
                let text = self.text;
                let mut whitespace_only = true;
                for c in text[self.start(value)..self.end(value)].chars() {
                    self.visit()?;
                    if !css_space(c) {
                        whitespace_only = false;
                        break;
                    }
                }
                if whitespace_only {
                    return Err(self.error(Error::ExpectedSyntax));
                }
            }
            value
        };
        self.separator()?;
        self.node(start, Kind::Declaration, vec![name, value])
            .map(Some)
    }

    /// Completes a style rule from its raw selector `header`, whose syntax
    /// starts at arena length `nodes` and comment count `comments`.
    fn style_rule(
        &mut self,
        start: usize,
        header: Id,
        nodes: usize,
        comments: usize,
        indent: usize,
        context: Context,
    ) -> Result<Id, SassError> {
        let selector = self.style_selector(header, nodes, comments, context.selectors)?;
        let mut children = vec![selector];
        children.extend(self.body(
            indent,
            Context {
                style: true,
                // Evaluation rejects style rules inside keyframe blocks and
                // cannot know the grammar of those from an include site.
                selectors: if context.selectors == SelectorContext::Normal {
                    SelectorContext::Normal
                } else {
                    SelectorContext::Unknown
                },
                ..context
            },
            true,
        )?);
        self.node(start, Kind::StyleRule, children)
    }

    pub(super) fn raw_header(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        loop {
            let (segment, comma) = self.almost_any_value()?;
            let segment_node = self.nodes.pop().unwrap();
            debug_assert_eq!(segment.0, self.nodes.len());
            parts.extend(segment_node.children);
            if self.syntax != SassSyntax::Indented || !comma || !self.peek().is_some_and(newline) {
                break;
            }
            self.bump()?;
        }
        self.node(start, Kind::InterpolatedText, parts)
    }
}
