//! Stylesheet statements and the two surface block grammars.

use super::{
    Parser, SassAtRule as At, SassError, SassErrorKind as Error, SassNodeId as Id,
    SassNodeKind as Kind, SassSyntax, canonical_name, css_space, decode_name, newline,
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
            if context.root && self.at("@charset") {
                let start = self.pos;
                self.expect("@")?;
                let name_start = self.pos;
                self.expect("charset")?;
                let name = self.node(name_start, Kind::Identifier, Vec::new())?;
                self.skip(false)?;
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
                _ => At::Css,
            };
            children.push(name);
            kind
        };
        if (matches!(kind, At::Use | At::Forward) && (!context.root || !use_allowed))
            || (kind == At::Return && !context.function)
            || (kind == At::Content && !context.mixin)
            || (kind == At::Extend && !context.style && !context.mixin && !context.content)
            || (matches!(kind, At::Else | At::ElseIf) && !allow_else)
            || (context.declaration
                && matches!(
                    kind,
                    At::Css
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
                    At::Css | At::Import | At::Include | At::Extend | At::AtRoot
                ))
        {
            return Err(self.error(Error::InvalidContext));
        }
        self.skip(self.syntax == SassSyntax::Scss)?;
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
                let plain = canonical_name(self.spelling(callable));
                if plain.starts_with("--") {
                    return Err(self.error(Error::UnsupportedSyntax));
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
                self.skip(self.syntax == SassSyntax::Scss)?;
                if self.at("(") {
                    children.push(self.arguments(true, false)?);
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
                    children.push(self.arguments(false, false)?);
                }
                self.skip(self.syntax == SassSyntax::Scss)?;
                if self.keyword("using")? {
                    self.skip(true)?;
                    children.push(self.arguments(true, false)?);
                    block = true;
                } else {
                    optional_block = true;
                }
            }
            At::Content => {
                if self.at("(") {
                    children.push(self.arguments(false, false)?);
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
                    children.push(self.raw_header(false)?.0);
                    // A query can leave the enclosing keyframes rule.
                    if context.selectors == SelectorContext::Keyframes {
                        child_context.selectors = SelectorContext::Unknown;
                    }
                } else if !self.at("{") && !self.peek().is_none_or(newline) {
                    // An inline selector is one style rule with its own body.
                    let rule_start = self.pos;
                    let nodes = self.nodes.len();
                    let comments = self.comments.len();
                    let header = self.raw_header(false)?.0;
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
                let header = self.raw_header(false)?.0;
                if self.start(header) == self.end(header) {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                children.push(header);
            }
            At::Import => {
                let header = self.raw_header(false)?.0;
                if self.start(header) == self.end(header) {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                children.push(header);
            }
            At::Css => {
                let name = children[0];
                children.push(self.raw_header(false)?.0);
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
        // A leading backslash forces a selector in indented Sass. Its bytes
        // belong to the rule, but not to the selector that follows it.
        let force_style =
            self.syntax == SassSyntax::Indented && !context.declaration && self.eat("\\")?;
        // Lookahead shares the real grammar and its work budget. It retains
        // no speculative nodes or comments and never guesses quote contexts.
        let nodes = self.nodes.len();
        let comments = self.comments.len();
        let (header, colon) = self.raw_header(false)?;
        let header_end = self.pos;
        let has_body = self.at("{");
        let first_colon = colon;
        let mut candidate = false;
        let allows_declarations =
            context.style || context.mixin || context.content || context.declaration;
        if first_colon.is_some() && allows_declarations && !force_style {
            let raw_nodes = self.nodes.len();
            let raw_comments = self.comments.len();
            let depth = self.depth;
            self.pos = start;
            if self.at("*") {
                self.bump()?;
            }
            match self.interpolated_name() {
                Ok(_) => {
                    self.skip(false)?;
                    candidate = Some(self.pos) == first_colon;
                }
                Err(error)
                    if matches!(
                        error.kind,
                        Error::NodeLimit | Error::DepthLimit | Error::WorkLimit
                    ) =>
                {
                    return Err(error);
                }
                Err(_) => {}
            }
            self.nodes.truncate(raw_nodes);
            self.comments.truncate(raw_comments);
            self.depth = depth;
            self.pos = header_end;
        }
        let declaration = if allows_declarations {
            candidate
                && first_colon.is_some_and(|colon| {
                    let after = &self.text[colon + 1..header_end];
                    if after.starts_with(':') {
                        return false;
                    }
                    if self.text[start..colon].trim().starts_with("--") {
                        return true;
                    }
                    let selector_prefix = self.name_start_at(colon + 1)
                        || after.starts_with("#{")
                        || after.starts_with("-#{");
                    // Indented Sass requires whitespace after a property
                    // colon when the value starts like a selector name.
                    (self.syntax == SassSyntax::Scss && !has_body)
                        || after.starts_with(css_space)
                        || !selector_prefix
                })
        } else {
            false
        };
        if !declaration {
            if context.declaration {
                return Err(self.error(Error::InvalidContext));
            }
            if self.start(header) == self.end(header) {
                return Err(self.error(Error::ExpectedSyntax));
            }
            return self.style_rule(start, header, nodes, comments, indent, context);
        }
        self.nodes.truncate(nodes);
        self.comments.truncate(comments);
        self.pos = start;
        if self.at("*") || self.at(":") {
            self.bump()?;
        }
        let name_start = self.pos;
        let name = self.interpolated_name()?;
        self.skip(false)?;
        self.expect(":")?;
        self.skip(self.syntax == SassSyntax::Scss)?;
        let mut children = vec![name];
        if self.text[name_start..self.end(name)].starts_with("--") {
            if context.declaration {
                return Err(self.error(Error::InvalidContext));
            }
            children.push(self.raw_header(true)?.0);
            self.separator()?;
        } else {
            if !self.at("{") && !self.peek().is_none_or(newline) && !self.at(";") && !self.at("}") {
                children.push(self.expression(true)?);
            }
            self.skip(self.syntax == SassSyntax::Scss)?;
            if has_body || (self.syntax == SassSyntax::Indented && self.peek().is_none_or(newline))
            {
                let body = self.body(
                    indent,
                    Context {
                        declaration: true,
                        ..context
                    },
                    has_body,
                )?;
                children.extend(body);
            } else {
                self.separator()?;
            }
        }
        if children.len() == 1 && !has_body {
            return Err(self.error(Error::ExpectedExpression));
        }
        self.node(start, Kind::Declaration, children)
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

    pub(super) fn raw_header(
        &mut self,
        curly_values: bool,
    ) -> Result<(Id, Option<usize>), SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        let mut colon = None;
        // Indented Sass continues after a trailing comma; comments between
        // the comma and the line end do not count, as Dart Sass omits them.
        let mut comma = false;
        while let Some(c) = self.peek() {
            self.visit()?;
            if matches!(c, ';' | '}') || (c == '{' && !curly_values) {
                break;
            }
            if self.syntax == SassSyntax::Indented && newline(c) && !comma {
                break;
            }
            if c == ':' && colon.is_none() {
                colon = Some(self.pos);
            }
            if !css_space(c) && !self.at("/*") && !(self.at("//") && !curly_values) {
                comma = c == ',';
            }
            if self.at("#{") {
                parts.push(self.interpolation()?);
            } else if c == '\'' || c == '"' {
                parts.push(self.quoted()?);
            } else if c == '(' || c == '[' || (c == '{' && curly_values) {
                parts.push(self.raw_group()?);
            } else if c == ')' || c == ']' {
                return Err(self.error(Error::ExpectedSyntax));
            } else if self.at("/*") {
                self.comment(false, false)?;
            } else if self.at("//") && !curly_values {
                self.comment(true, false)?;
            } else if c == '\\' {
                self.escape(false)?;
            } else {
                self.bump()?;
            }
        }
        let id = self.node(start, Kind::InterpolatedText, parts)?;
        Ok((id, colon))
    }
}
