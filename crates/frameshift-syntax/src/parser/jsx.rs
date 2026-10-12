//! JSX elements and fragments, following the JSX specification grammar.
//!
//! Tags are token sequences with ordinary JavaScript trivia between tokens,
//! but their punctuation is read byte by byte, because the JavaScript
//! punctuator scanner would join `>` with a following `=` or `>`. Children are
//! not trivia: text runs from the end of a tag or container to the next `{` or
//! `<`, so the parser never skips trivia at the start of a child.

use frameshift_core::Span;

use crate::{
    ast::{ChildList, NodeId, SyntaxNodeKind},
    jsx::{scan_jsx_identifier, scan_jsx_string, scan_jsx_text},
    punctuator::Punctuator,
};

use super::{ParseError, ParseErrorKind, Parser, parse_lexical_error};

impl<'source> Parser<'source> {
    // Parses an element or fragment as a primary expression. The grammar
    // would read a following `<` as a comparison, but Babel and TypeScript
    // reject adjacent elements without a wrapper, so no compiler accepts
    // such a file.
    pub(super) fn parse_jsx_expression(&mut self) -> Result<NodeId, ParseError> {
        let element = self.parse_jsx_element()?;
        if let Some(token) = self.peek_punctuator(self.goal_after_expression())?
            && token.kind == Punctuator::LessThan
        {
            return Err(self.error(token.span.start(), ParseErrorKind::AdjacentJsxElements));
        }
        Ok(element)
    }

    // Parses an element or fragment whose `<` is the next token.
    pub(super) fn parse_jsx_element(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        self.consume_jsx_byte(b'<')?;
        self.parse_jsx_after_less_than(start)
    }

    // Parses the rest of an element or fragment after its consumed `<`.
    fn parse_jsx_after_less_than(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.parse_jsx_after_less_than_inner(start);
        self.leave();
        result
    }

    fn parse_jsx_after_less_than_inner(&mut self, start: usize) -> Result<NodeId, ParseError> {
        if self.eat_jsx_byte(b'>')? {
            let opening =
                self.add_node(SyntaxNodeKind::JsxOpeningFragment, self.span_from(start)?)?;
            let (children, closing_start) = self.parse_jsx_children(start)?;
            let (closing, closing_name) = self.parse_jsx_closing(closing_start)?;
            if closing_name.is_some() {
                return Err(self.error(closing_start, ParseErrorKind::JsxClosingTagMismatch));
            }
            return self.add_node(
                SyntaxNodeKind::JsxFragment {
                    opening,
                    children,
                    closing,
                },
                self.span_from(start)?,
            );
        }
        let name = self.parse_jsx_element_name()?;
        let attributes = self.parse_jsx_attributes()?;
        let self_closing = self.eat_jsx_byte(b'/')?;
        if !self.eat_jsx_byte(b'>')? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedJsxTagEnd));
        }
        let opening = self.add_node(
            SyntaxNodeKind::JsxOpeningElement {
                name,
                attributes,
                self_closing,
            },
            self.span_from(start)?,
        )?;
        let (children, closing) = if self_closing {
            (self.append_children(&[])?, None)
        } else {
            let (children, closing_start) = self.parse_jsx_children(start)?;
            let (closing, closing_name) = self.parse_jsx_closing(closing_start)?;
            if !closing_name.is_some_and(|closing_name| self.jsx_names_match(name, closing_name)) {
                return Err(self.error(closing_start, ParseErrorKind::JsxClosingTagMismatch));
            }
            (children, Some(closing))
        };
        self.add_node(
            SyntaxNodeKind::JsxElement {
                opening,
                children,
                closing,
            },
            self.span_from(start)?,
        )
    }

    // Parses children up to and including the `</` of the closing tag, and
    // returns them with the offset of that `<`.
    fn parse_jsx_children(
        &mut self,
        element_start: usize,
    ) -> Result<(ChildList, usize), ParseError> {
        let mut children = self.take_list();
        let closing_start = loop {
            match self.text.as_bytes().get(self.cursor) {
                None => {
                    return Err(self.error(element_start, ParseErrorKind::UnclosedJsxElement));
                }
                Some(b'<') => {
                    let start = self.cursor;
                    self.consume_jsx_byte(b'<')?;
                    if self.eat_jsx_byte(b'/')? {
                        break start;
                    }
                    let child = self.parse_jsx_after_less_than(start)?;
                    self.push_child(&mut children, child)?;
                }
                Some(b'{') => {
                    let child = self.parse_jsx_child_container()?;
                    self.push_child(&mut children, child)?;
                }
                Some(_) => {
                    let span = scan_jsx_text(self.source, self.cursor, self.limits.lexical)
                        .map_err(parse_lexical_error)?;
                    self.consume_token(span)?;
                    let child = self.add_node(SyntaxNodeKind::JsxText, span)?;
                    self.push_child(&mut children, child)?;
                }
            }
        };
        Ok((self.finish_list(children)?, closing_start))
    }

    // Parses `{}`, `{expression}`, or `{...expression}` among children.
    fn parse_jsx_child_container(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.consume_jsx_byte(b'{')?;
        self.within_braces(|parser| {
            let kind = if parser.at_punctuator(Punctuator::RightBrace)? {
                SyntaxNodeKind::JsxExpressionContainer { expression: None }
            } else if parser
                .consume_punctuator(Punctuator::Ellipsis, parser.goal_for_expression_start())?
                .is_some()
            {
                SyntaxNodeKind::JsxSpreadChild {
                    expression: parser.parse_expression(2)?,
                }
            } else {
                SyntaxNodeKind::JsxExpressionContainer {
                    expression: Some(parser.parse_expression(2)?),
                }
            };
            parser.expect_punctuator(Punctuator::RightBrace, parser.goal_after_expression())?;
            parser.add_node(kind, parser.span_from(start)?)
        })
    }

    // Parses the closing tag after its consumed `</`, returning the closing
    // node and, for an element, its name.
    fn parse_jsx_closing(&mut self, start: usize) -> Result<(NodeId, Option<NodeId>), ParseError> {
        if self.eat_jsx_byte(b'>')? {
            let closing =
                self.add_node(SyntaxNodeKind::JsxClosingFragment, self.span_from(start)?)?;
            return Ok((closing, None));
        }
        let name = self.parse_jsx_element_name()?;
        if !self.eat_jsx_byte(b'>')? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedJsxTagEnd));
        }
        let closing = self.add_node(
            SyntaxNodeKind::JsxClosingElement { name },
            self.span_from(start)?,
        )?;
        Ok((closing, Some(name)))
    }

    // JSXElementName: an identifier, a namespaced name, or a member chain.
    fn parse_jsx_element_name(&mut self) -> Result<NodeId, ParseError> {
        let name = self.parse_jsx_namespaced_name()?;
        if !matches!(self.node_kind(name), SyntaxNodeKind::JsxIdentifier) {
            return Ok(name);
        }
        let start = self.node_span(name).start();
        let mut object = name;
        while self.eat_jsx_byte(b'.')? {
            let property = self.parse_jsx_identifier()?;
            object = self.add_node(
                SyntaxNodeKind::JsxMemberExpression { object, property },
                self.span_from(start)?,
            )?;
        }
        Ok(object)
    }

    // JSXAttributeName, and the first part of a JSXElementName.
    fn parse_jsx_namespaced_name(&mut self) -> Result<NodeId, ParseError> {
        let namespace = self.parse_jsx_identifier()?;
        if !self.eat_jsx_byte(b':')? {
            return Ok(namespace);
        }
        let name = self.parse_jsx_identifier()?;
        let span = self.span_from(self.node_span(namespace).start())?;
        self.add_node(SyntaxNodeKind::JsxNamespacedName { namespace, name }, span)
    }

    fn parse_jsx_identifier(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let span = scan_jsx_identifier(self.source, self.cursor, self.limits.lexical)
            .map_err(parse_lexical_error)?;
        self.consume_token(span)?;
        self.add_node(SyntaxNodeKind::JsxIdentifier, span)
    }

    // Parses attributes until `/` or `>`, which stays unconsumed.
    fn parse_jsx_attributes(&mut self) -> Result<ChildList, ParseError> {
        let mut attributes = self.take_list();
        loop {
            let attribute = match self.peek_jsx_byte()? {
                Some(b'/' | b'>') => break,
                Some(b'{') => self.parse_jsx_spread_attribute()?,
                Some(_) if self.identifier_can_start_here()? => self.parse_jsx_attribute()?,
                Some(_) | None => {
                    return Err(self.error(self.cursor, ParseErrorKind::ExpectedJsxTagEnd));
                }
            };
            self.push_child(&mut attributes, attribute)?;
        }
        self.finish_list(attributes)
    }

    fn parse_jsx_spread_attribute(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.consume_jsx_byte(b'{')?;
        self.within_braces(|parser| {
            parser.expect_punctuator(Punctuator::Ellipsis, parser.goal_for_expression_start())?;
            let argument = parser.parse_expression(2)?;
            parser.expect_punctuator(Punctuator::RightBrace, parser.goal_after_expression())?;
            parser.add_node(
                SyntaxNodeKind::JsxSpreadAttribute { argument },
                parser.span_from(start)?,
            )
        })
    }

    fn parse_jsx_attribute(&mut self) -> Result<NodeId, ParseError> {
        let name = self.parse_jsx_namespaced_name()?;
        let start = self.node_span(name).start();
        let value = if self.eat_jsx_byte(b'=')? {
            Some(self.parse_jsx_attribute_value()?)
        } else {
            None
        };
        self.add_node(
            SyntaxNodeKind::JsxAttribute { name, value },
            self.span_from(start)?,
        )
    }

    fn parse_jsx_attribute_value(&mut self) -> Result<NodeId, ParseError> {
        match self.peek_jsx_byte()? {
            Some(b'"' | b'\'') => {
                let token = scan_jsx_string(self.source, self.cursor, self.limits.lexical)
                    .map_err(parse_lexical_error)?;
                self.consume_token(token.span)?;
                self.add_node(SyntaxNodeKind::JsxStringLiteral, token.span)
            }
            Some(b'{') => {
                let start = self.cursor;
                self.consume_jsx_byte(b'{')?;
                self.within_braces(|parser| {
                    if parser.at_punctuator(Punctuator::RightBrace)? {
                        return Err(parser.error(start, ParseErrorKind::EmptyJsxAttributeValue));
                    }
                    let expression = parser.parse_expression(2)?;
                    parser.expect_punctuator(
                        Punctuator::RightBrace,
                        parser.goal_after_expression(),
                    )?;
                    parser.add_node(
                        SyntaxNodeKind::JsxExpressionContainer {
                            expression: Some(expression),
                        },
                        parser.span_from(start)?,
                    )
                })
            }
            Some(b'<') => self.parse_jsx_element(),
            Some(_) | None => {
                Err(self.error(self.cursor, ParseErrorKind::ExpectedJsxAttributeValue))
            }
        }
    }

    // Closing names match when they have the same structure and spellings,
    // regardless of trivia between their parts.
    fn jsx_names_match(&self, opening: NodeId, closing: NodeId) -> bool {
        match (self.node_kind(opening), self.node_kind(closing)) {
            (SyntaxNodeKind::JsxIdentifier, SyntaxNodeKind::JsxIdentifier) => {
                self.slice(self.node_span(opening)) == self.slice(self.node_span(closing))
            }
            (
                SyntaxNodeKind::JsxNamespacedName {
                    namespace: left_namespace,
                    name: left_name,
                },
                SyntaxNodeKind::JsxNamespacedName {
                    namespace: right_namespace,
                    name: right_name,
                },
            ) => {
                self.jsx_names_match(left_namespace, right_namespace)
                    && self.jsx_names_match(left_name, right_name)
            }
            (
                SyntaxNodeKind::JsxMemberExpression {
                    object: left_object,
                    property: left_property,
                },
                SyntaxNodeKind::JsxMemberExpression {
                    object: right_object,
                    property: right_property,
                },
            ) => {
                self.jsx_names_match(left_object, right_object)
                    && self.jsx_names_match(left_property, right_property)
            }
            (_, _) => false,
        }
    }

    // The next byte of a tag after trivia. Tag punctuation is ASCII.
    fn peek_jsx_byte(&mut self) -> Result<Option<u8>, ParseError> {
        self.skip_trivia()?;
        Ok(self.text.as_bytes().get(self.cursor).copied())
    }

    fn eat_jsx_byte(&mut self, byte: u8) -> Result<bool, ParseError> {
        if self.peek_jsx_byte()? != Some(byte) {
            return Ok(false);
        }
        self.consume_jsx_byte(byte)?;
        Ok(true)
    }

    // Consumes one ASCII tag or container byte at the cursor, which the caller
    // has checked.
    fn consume_jsx_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        debug_assert_eq!(self.text.as_bytes().get(self.cursor), Some(&byte));
        let span = Span::new(self.cursor, self.cursor + 1)
            .map_err(|error| self.error(self.cursor, ParseErrorKind::InternalInvariant(error)))?;
        self.consume_token(span)
    }
}
