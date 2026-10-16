//! Import and export declarations at the top level of a module.

use frameshift_core::Span;

use super::{
    ParseError, ParseErrorKind, Parser, Strictness, class::ClassPlacement,
    function::FunctionPlacement, names::Name, scope::Declaration,
};
use crate::{
    ast::{FunctionKind, NodeId, SyntaxNodeKind},
    identifier::{IdentifierKind, ReservedWordClass},
    lookup::{LookupMap, LookupSet},
    punctuator::Punctuator,
    value::{
        identifier_chars, string_literal_is_well_formed, string_literal_units, string_literal_value,
    },
};

#[derive(Default)]
pub(super) struct ModuleState<'source> {
    // Exported names by StringValue, which must be unique.
    exported_names: LookupMap<String, usize>,
    // Local bindings exported by `export { name }`, checked when the module ends.
    exported_bindings: Vec<(Name<'source>, usize)>,
}

// A parsed `ModuleExportName` with its StringValue.
struct ExportName {
    node: NodeId,
    value: String,
    is_string: bool,
}

impl<'source> Parser<'source> {
    // After the module body: every `export { name }` names a top-level binding.
    pub(super) fn check_exported_bindings(&self) -> Result<(), ParseError> {
        if self.typescript {
            return Ok(());
        }
        let undeclared = self
            .module
            .exported_bindings
            .iter()
            .filter(|(name, _)| !self.module_scope_declares(name))
            .map(|&(_, offset)| offset)
            .min();
        match undeclared {
            Some(offset) => Err(self.error(offset, ParseErrorKind::UndeclaredExport)),
            None => Ok(()),
        }
    }

    pub(super) fn parse_import_declaration(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let type_only = self.typescript && self.ts_import_type_only()?;
        if type_only {
            self.scan_identifier()?;
        }
        let mut specifiers = Vec::new();
        if !matches!(self.peek_char()?, Some('"' | '\'')) {
            let mut clause_needed = true;
            if self.identifier_can_start_here()? {
                let local = self.parse_imported_binding()?;
                let span = self.node_span(local);
                let specifier =
                    self.add_node(SyntaxNodeKind::ImportDefaultSpecifier { local }, span)?;
                self.push_child(&mut specifiers, specifier)?;
                clause_needed = self
                    .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                    .is_some();
            }
            if clause_needed {
                self.skip_trivia()?;
                let clause_start = self.cursor;
                if self
                    .consume_punctuator(Punctuator::Star, self.goal_after_expression())?
                    .is_some()
                {
                    if self.consume_word("as")?.is_none() {
                        return Err(self.error(self.cursor, ParseErrorKind::ExpectedAs));
                    }
                    let local = self.parse_imported_binding()?;
                    let span = self.span_from(clause_start)?;
                    let specifier =
                        self.add_node(SyntaxNodeKind::ImportNamespaceSpecifier { local }, span)?;
                    self.push_child(&mut specifiers, specifier)?;
                } else if self.at_char('{')? {
                    self.within_braces(|parser| parser.parse_named_imports(&mut specifiers))?;
                } else {
                    return Err(self.error(
                        self.cursor,
                        ParseErrorKind::ExpectedPunctuator(Punctuator::LeftBrace),
                    ));
                }
            }
            if self.consume_word("from")?.is_none() {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedFrom));
            }
        }
        let source = self.parse_module_specifier()?;
        let attributes = self.parse_import_attributes()?;
        self.consume_semicolon()?;
        let specifiers = self.append_children(&specifiers)?;
        let span = self.span_from(start)?;
        let declaration = self.add_node(
            SyntaxNodeKind::ImportDeclaration {
                specifiers,
                source,
                attributes,
            },
            span,
        )?;
        if type_only {
            self.ts_node(crate::TypeScriptKind::TypeOnly, start, &[declaration])
        } else {
            Ok(declaration)
        }
    }

    fn parse_named_imports(&mut self, specifiers: &mut Vec<NodeId>) -> Result<(), ParseError> {
        self.expect_punctuator(Punctuator::LeftBrace, self.goal_after_expression())?;
        loop {
            if self.at_char('}')? {
                break;
            }
            self.skip_trivia()?;
            let start = self.cursor;
            let type_only = self.typescript && self.ts_specifier_type_only()?;
            if type_only {
                self.scan_identifier()?;
            }
            let specifier = if self.next_named_import_has_alias()? {
                let imported = self.parse_module_export_name()?;
                if self.consume_word("as")?.is_none() {
                    return Err(self.error(self.cursor, ParseErrorKind::ExpectedAs));
                }
                let local = self.parse_imported_binding()?;
                let span = self.span_from(start)?;
                self.add_node(
                    SyntaxNodeKind::ImportSpecifier {
                        imported: Some(imported.node),
                        local,
                    },
                    span,
                )?
            } else {
                if !self.identifier_can_start_here()? {
                    return Err(self.error(self.cursor, ParseErrorKind::ExpectedAs));
                }
                let local = self.parse_imported_binding()?;
                let span = self.node_span(local);
                self.add_node(
                    SyntaxNodeKind::ImportSpecifier {
                        imported: None,
                        local,
                    },
                    span,
                )?
            };
            let specifier = if type_only {
                self.ts_node(crate::TypeScriptKind::TypeOnly, start, &[specifier])?
            } else {
                specifier
            };
            self.push_child(specifiers, specifier)?;
            if self
                .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                .is_none()
            {
                break;
            }
        }
        self.expect_punctuator(Punctuator::RightBrace, self.goal_after_expression())?;
        Ok(())
    }

    // Reports whether the next named import is `name as local`.
    fn next_named_import_has_alias(&mut self) -> Result<bool, ParseError> {
        self.lookahead(|parser| {
            if matches!(parser.peek_char()?, Some('"' | '\'')) {
                return Ok(true);
            }
            if !parser.identifier_can_start_here()? {
                return Ok(false);
            }
            parser.scan_identifier()?;
            parser.at_word("as")
        })
    }

    // An `ImportedBinding`: a lexically declared binding identifier.
    fn parse_imported_binding(&mut self) -> Result<NodeId, ParseError> {
        if !self.identifier_can_start_here()? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
        }
        let token = self.scan_identifier()?;
        if token.kind == IdentifierKind::PrivateName {
            return Err(self.error(token.span.start(), ParseErrorKind::UnexpectedPrivateName));
        }
        self.check_name(token)?;
        self.check_restricted_name(token.span, Strictness::Strict)?;
        let name = self.name_of(token.span);
        if name == "let" {
            return Err(self.error(token.span.start(), ParseErrorKind::LetBindingName));
        }
        self.declare(name, token.span.start(), Declaration::Lexical)?;
        self.add_node(
            SyntaxNodeKind::BindingIdentifier {
                has_escape: token.has_escape,
            },
            token.span,
        )
    }

    fn parse_module_specifier(&mut self) -> Result<NodeId, ParseError> {
        if !matches!(self.peek_char()?, Some('"' | '\'')) {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedModuleSpecifier));
        }
        Ok(self.parse_string()?.0)
    }

    // A `ModuleExportName`: any identifier name, or a string without
    // unpaired surrogates.
    fn parse_module_export_name(&mut self) -> Result<ExportName, ParseError> {
        if matches!(self.peek_char()?, Some('"' | '\'')) {
            let (node, token) = self.parse_string()?;
            let spelling = self.slice(token.span);
            if !string_literal_is_well_formed(spelling) {
                return Err(self.error(token.span.start(), ParseErrorKind::InvalidModuleExportName));
            }
            return Ok(ExportName {
                node,
                value: string_literal_value(spelling),
                is_string: true,
            });
        }
        if !self.identifier_can_start_here()? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
        }
        let token = self.scan_identifier()?;
        if token.kind == IdentifierKind::PrivateName {
            return Err(self.error(token.span.start(), ParseErrorKind::UnexpectedPrivateName));
        }
        let node = self.add_node(
            SyntaxNodeKind::IdentifierName {
                has_escape: token.has_escape,
                reserved_word: token.reserved_word,
            },
            token.span,
        )?;
        Ok(ExportName {
            node,
            value: identifier_chars(self.slice(token.span)).collect(),
            is_string: false,
        })
    }

    // `with { key: "value", ... }`, if present.
    fn parse_import_attributes(&mut self) -> Result<Option<NodeId>, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        if self.consume_word("with")?.is_none() {
            return Ok(None);
        }
        let attributes = self.within_braces(|parser| {
            parser.expect_punctuator(Punctuator::LeftBrace, parser.goal_after_expression())?;
            let mut attributes = Vec::new();
            let mut keys = LookupSet::default();
            loop {
                if parser.at_char('}')? {
                    break;
                }
                parser.skip_trivia()?;
                let key_start = parser.cursor;
                let (key, units): (NodeId, Vec<u16>) =
                    if matches!(parser.peek_char()?, Some('"' | '\'')) {
                        let (node, token) = parser.parse_string()?;
                        (
                            node,
                            string_literal_units(parser.slice(token.span)).collect(),
                        )
                    } else if parser.identifier_can_start_here()? {
                        let token = parser.scan_identifier()?;
                        if token.kind == IdentifierKind::PrivateName {
                            return Err(parser
                                .error(token.span.start(), ParseErrorKind::UnexpectedPrivateName));
                        }
                        let units = identifier_chars(parser.slice(token.span))
                            .collect::<String>()
                            .encode_utf16()
                            .collect();
                        let node = parser.add_node(
                            SyntaxNodeKind::IdentifierName {
                                has_escape: token.has_escape,
                                reserved_word: token.reserved_word,
                            },
                            token.span,
                        )?;
                        (node, units)
                    } else {
                        return Err(parser.error(key_start, ParseErrorKind::ExpectedIdentifier));
                    };
                if !keys.insert(units) {
                    return Err(parser.error(key_start, ParseErrorKind::DuplicateImportAttribute));
                }
                parser.expect_punctuator(Punctuator::Colon, parser.goal_after_expression())?;
                if !matches!(parser.peek_char()?, Some('"' | '\'')) {
                    return Err(
                        parser.error(parser.cursor, ParseErrorKind::ExpectedModuleSpecifier)
                    );
                }
                let (value, _) = parser.parse_string()?;
                let span = parser.span_from(key_start)?;
                let attribute =
                    parser.add_node(SyntaxNodeKind::ImportAttribute { key, value }, span)?;
                parser.push_child(&mut attributes, attribute)?;
                if parser
                    .consume_punctuator(Punctuator::Comma, parser.goal_after_expression())?
                    .is_none()
                {
                    break;
                }
            }
            parser.expect_punctuator(Punctuator::RightBrace, parser.goal_after_expression())?;
            parser.append_children(&attributes)
        })?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::ImportAttributes { attributes }, span)
            .map(Some)
    }

    fn register_export(&mut self, name: String, offset: usize) -> Result<(), ParseError> {
        if self.typescript {
            return Ok(());
        }
        if self.module.exported_names.contains_key(&name) {
            return Err(self.error(offset, ParseErrorKind::DuplicateExport));
        }
        self.module.exported_names.insert(name, offset);
        Ok(())
    }

    pub(super) fn parse_export_declaration(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        if self.typescript {
            if self.ts_symbol('=')? {
                let expression = self.parse_assignment_with_in()?;
                self.consume_semicolon()?;
                return self.ts_node(
                    crate::TypeScriptKind::ExportAssignment,
                    start,
                    &[expression],
                );
            }
            if self.consume_word("as")?.is_some() {
                if self.consume_word("namespace")?.is_none() {
                    return Err(self.error(self.cursor, ParseErrorKind::InvalidTypeScriptSyntax));
                }
                let name = self.ts_name(true)?;
                self.consume_semicolon()?;
                return self.ts_node(crate::TypeScriptKind::NamespaceExport, start, &[name]);
            }
            if self.at_word("type")?
                && self.lookahead(|parser| {
                    parser.scan_identifier()?;
                    Ok(parser.at_char('{')? || parser.at_char('*')?)
                })?
            {
                self.scan_identifier()?;
                let declaration = if self.ts_symbol('*')? {
                    self.parse_export_all(start)?
                } else {
                    self.parse_export_named(start)?
                };
                return self.ts_node(crate::TypeScriptKind::TypeOnly, start, &[declaration]);
            }
        }
        if self
            .consume_punctuator(Punctuator::Star, self.goal_after_expression())?
            .is_some()
        {
            return self.parse_export_all(start);
        }
        if self.at_char('{')? {
            return self.parse_export_named(start);
        }
        self.skip_trivia()?;
        let declaration_start = self.cursor;
        if self.consume_word("default")?.is_some() {
            self.register_export("default".to_owned(), declaration_start)?;
            let declaration = self.parse_export_default()?;
            let span = self.span_from(start)?;
            return self.add_node(SyntaxNodeKind::ExportDefault { declaration }, span);
        }
        let declaration = self.parse_exported_declaration(declaration_start)?;
        let mut names = Vec::new();
        self.collect_declared_names(declaration, &mut names)?;
        for span in names {
            let name = self.name_of(span).into_owned();
            self.register_export(name, span.start())?;
        }
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::ExportDeclaration { declaration }, span)
    }

    fn parse_export_all(&mut self, start: usize) -> Result<NodeId, ParseError> {
        let exported = if self.consume_word("as")?.is_some() {
            let name = self.parse_module_export_name()?;
            let offset = self.node_span(name.node).start();
            self.register_export(name.value, offset)?;
            Some(name.node)
        } else {
            None
        };
        if self.consume_word("from")?.is_none() {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedFrom));
        }
        let source = self.parse_module_specifier()?;
        let attributes = self.parse_import_attributes()?;
        self.consume_semicolon()?;
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::ExportAll {
                exported,
                source,
                attributes,
            },
            span,
        )
    }

    fn parse_export_named(&mut self, start: usize) -> Result<NodeId, ParseError> {
        let mut specifiers = Vec::new();
        let mut locals = Vec::new();
        self.within_braces(|parser| {
            parser.expect_punctuator(Punctuator::LeftBrace, parser.goal_after_expression())?;
            loop {
                if parser.at_char('}')? {
                    break;
                }
                parser.skip_trivia()?;
                let specifier_start = parser.cursor;
                let type_only = parser.typescript && parser.ts_specifier_type_only()?;
                if type_only {
                    parser.scan_identifier()?;
                }
                let local = parser.parse_module_export_name()?;
                let exported = if parser.consume_word("as")?.is_some() {
                    Some(parser.parse_module_export_name()?)
                } else {
                    None
                };
                let span = parser.span_from(specifier_start)?;
                let specifier = parser.add_node(
                    SyntaxNodeKind::ExportSpecifier {
                        local: local.node,
                        exported: exported.as_ref().map(|name| name.node),
                    },
                    span,
                )?;
                let specifier = if type_only {
                    parser.ts_node(
                        crate::TypeScriptKind::TypeOnly,
                        specifier_start,
                        &[specifier],
                    )?
                } else {
                    specifier
                };
                parser.push_child(&mut specifiers, specifier)?;
                locals
                    .try_reserve(1)
                    .map_err(|_| parser.error(specifier_start, ParseErrorKind::AllocationFailed))?;
                locals.push((local, exported));
                if parser
                    .consume_punctuator(Punctuator::Comma, parser.goal_after_expression())?
                    .is_none()
                {
                    break;
                }
            }
            parser.expect_punctuator(Punctuator::RightBrace, parser.goal_after_expression())?;
            Ok(())
        })?;
        let (source, attributes) = if self.consume_word("from")?.is_some() {
            let source = self.parse_module_specifier()?;
            (Some(source), self.parse_import_attributes()?)
        } else {
            (None, None)
        };
        for (local, exported) in locals {
            let local_offset = self.node_span(local.node).start();
            if source.is_none() {
                self.export_local_binding(&local)?;
            }
            match exported {
                Some(exported) => {
                    let offset = self.node_span(exported.node).start();
                    self.register_export(exported.value, offset)?;
                }
                None => self.register_export(local.value, local_offset)?,
            }
        }
        self.consume_semicolon()?;
        let specifiers = self.append_children(&specifiers)?;
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::ExportNamed {
                specifiers,
                source,
                attributes,
            },
            span,
        )
    }

    // Without `from`, an export specifier refers to a local binding, which
    // must be an identifier that could be declared, and becomes a reference.
    fn export_local_binding(&mut self, local: &ExportName) -> Result<(), ParseError> {
        let span = self.node_span(local.node);
        let invalid = self.error(span.start(), ParseErrorKind::InvalidLocalExport);
        if local.is_string {
            return Err(invalid);
        }
        let SyntaxNodeKind::IdentifierName {
            has_escape,
            reserved_word,
        } = self.node_kind(local.node)
        else {
            return Err(invalid);
        };
        if reserved_word.is_some_and(|word| {
            matches!(
                word.class(),
                ReservedWordClass::Always
                    | ReservedWordClass::Contextual
                    | ReservedWordClass::Strict
            )
        }) {
            return Err(invalid);
        }
        self.set_kind(
            local.node,
            SyntaxNodeKind::Identifier {
                has_escape,
                reserved_word,
            },
        );
        let name = self.name_of(span);
        self.module
            .exported_bindings
            .try_reserve(1)
            .map_err(|_| self.error(span.start(), ParseErrorKind::AllocationFailed))?;
        self.module.exported_bindings.push((name, span.start()));
        Ok(())
    }

    // `export default` followed by a function or class declaration, or an
    // AssignmentExpression and a semicolon.
    fn parse_export_default(&mut self) -> Result<NodeId, ParseError> {
        if self.typescript
            && let Some(declaration) = self.ts_declaration(true)?
        {
            return Ok(declaration);
        }
        self.skip_trivia()?;
        let start = self.cursor;
        if self.consume_word("function")?.is_some() {
            let kind = if self
                .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
                .is_some()
            {
                FunctionKind::Generator
            } else {
                FunctionKind::Normal
            };
            return self.parse_function(start, kind, FunctionPlacement::ExportDefault);
        }
        if let Some(token) = self.peek_name()?
            && self.is_unescaped(token, "async")
        {
            let function = self.lookahead(|parser| {
                parser.consume_token(token.span)?;
                Ok(!parser.line_break_before_next()? && parser.at_word("function")?)
            })?;
            if function {
                self.consume_token(token.span)?;
                self.scan_identifier()?;
                let kind = if self
                    .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
                    .is_some()
                {
                    FunctionKind::AsyncGenerator
                } else {
                    FunctionKind::Async
                };
                return self.parse_function(start, kind, FunctionPlacement::ExportDefault);
            }
        }
        if self.consume_word("class")?.is_some() {
            return self.parse_class(start, ClassPlacement::ExportDefault);
        }
        let expression = self.parse_assignment_with_in()?;
        self.consume_semicolon()?;
        Ok(expression)
    }

    // `export` followed by `var`, `let`, `const`, a function, or a class.
    fn parse_exported_declaration(&mut self, start: usize) -> Result<NodeId, ParseError> {
        if self.typescript
            && let Some(declaration) = self.ts_declaration(true)?
        {
            return Ok(declaration);
        }
        let Some(token) = self.peek_name()? else {
            return Err(self.error(start, ParseErrorKind::ExpectedExpression));
        };
        if self.is_unescaped(token, "using") {
            return Err(self.error(start, ParseErrorKind::InvalidUsingDeclaration));
        }
        let exportable = !token.has_escape
            && token.kind == IdentifierKind::Name
            && matches!(
                self.slice(token.span),
                "var" | "let" | "const" | "function" | "async" | "class"
            );
        if !exportable {
            return Err(self.error(start, ParseErrorKind::ExpectedExpression));
        }
        let declaration = self.parse_module_item_declaration()?;
        if !matches!(
            self.node_kind(declaration),
            SyntaxNodeKind::VariableDeclaration { .. }
                | SyntaxNodeKind::FunctionDeclaration { .. }
                | SyntaxNodeKind::ClassDeclaration { .. }
        ) {
            return Err(self.error(start, ParseErrorKind::ExpectedExpression));
        }
        if let SyntaxNodeKind::VariableDeclaration { kind, .. } = self.node_kind(declaration)
            && matches!(
                kind,
                crate::ast::VariableKind::Using | crate::ast::VariableKind::AwaitUsing
            )
        {
            return Err(self.error(start, ParseErrorKind::InvalidUsingDeclaration));
        }
        Ok(declaration)
    }

    // Collects the names a declaration binds, walking patterns but not
    // default values or computed keys.
    fn collect_declared_names(&self, id: NodeId, names: &mut Vec<Span>) -> Result<(), ParseError> {
        let kind = self.node_kind(id);
        let push = |names: &mut Vec<Span>, span: Span| {
            names
                .try_reserve(1)
                .map_err(|_| self.error(span.start(), ParseErrorKind::AllocationFailed))?;
            names.push(span);
            Ok::<(), ParseError>(())
        };
        if let SyntaxNodeKind::BindingIdentifier { .. } = kind {
            return push(names, self.node_span(id));
        }
        if let SyntaxNodeKind::VariableDeclaration { declarators, .. } = kind {
            for index in declarators.range() {
                self.collect_declared_names(self.children[index], names)?;
            }
        } else if let SyntaxNodeKind::VariableDeclarator { target, .. } = kind {
            self.collect_declared_names(target, names)?;
        } else if let SyntaxNodeKind::FunctionDeclaration {
            name: Some(name), ..
        }
        | SyntaxNodeKind::ClassDeclaration {
            name: Some(name), ..
        } = kind
        {
            push(names, self.node_span(name))?;
        } else if let SyntaxNodeKind::ArrayPattern { elements, .. } = kind {
            for index in elements.range() {
                self.collect_declared_names(self.children[index], names)?;
            }
        } else if let SyntaxNodeKind::ObjectPattern { properties, .. } = kind {
            for index in properties.range() {
                self.collect_declared_names(self.children[index], names)?;
            }
        } else if let SyntaxNodeKind::PatternProperty { value, .. } = kind {
            self.collect_declared_names(value, names)?;
        } else if let SyntaxNodeKind::ShorthandPatternProperty { target, .. }
        | SyntaxNodeKind::AssignmentPattern { target, .. } = kind
        {
            self.collect_declared_names(target, names)?;
        } else if let SyntaxNodeKind::RestElement { argument } = kind {
            self.collect_declared_names(argument, names)?;
        }
        Ok(())
    }
}
