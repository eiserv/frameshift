use std::borrow::Cow;

use frameshift_core::Span;

use super::{Goal, ParseError, ParseErrorKind, Parser, StrictModeError, Strictness};
use crate::{
    identifier::{IdentifierKind, IdentifierToken, ReservedWord, ReservedWordClass},
    value::{identifier_chars, identifier_equals},
};

/// The StringValue of an identifier name: borrowed unless it contains escapes.
pub(super) type Name<'source> = Cow<'source, str>;

impl<'source> Parser<'source> {
    // Decodes the StringValue of an accepted identifier spelling, without `#`.
    pub(super) fn name_of(&self, span: Span) -> Name<'source> {
        let spelling = self.slice(span);
        if spelling.contains('\\') {
            Cow::Owned(identifier_chars(spelling).collect())
        } else {
            Cow::Borrowed(spelling)
        }
    }

    // The reserved word whose spelling equals the StringValue of a name span.
    pub(super) fn reserved_word_of(&self, span: Span) -> Option<ReservedWord> {
        ReservedWord::from_spelling(&self.name_of(span))
    }

    // Applies the context rules for `IdentifierReference`, `BindingIdentifier`,
    // and `LabelIdentifier`, which share their treatment of reserved words.
    // Strictness rules for binding `eval` and `arguments` are separate,
    // because parameter lists learn their strictness from the body.
    pub(super) fn check_name(&mut self, token: IdentifierToken) -> Result<(), ParseError> {
        let offset = token.span.start();
        if token.kind == IdentifierKind::PrivateName {
            return Err(self.error(offset, ParseErrorKind::UnexpectedPrivateName));
        }
        let Some(word) = token.reserved_word else {
            return Ok(());
        };
        match word.class() {
            ReservedWordClass::Always => {
                let kind = if token.has_escape {
                    ParseErrorKind::EscapedReservedWord
                } else {
                    ParseErrorKind::ReservedWord
                };
                Err(self.error(offset, kind))
            }
            ReservedWordClass::Contextual if word == ReservedWord::Yield => {
                if self.context.yield_keyword {
                    return Err(self.error(offset, ParseErrorKind::ReservedWord));
                }
                match self.context.strict {
                    Strictness::Sloppy => Ok(()),
                    Strictness::Strict => Err(self.error(offset, ParseErrorKind::ReservedWord)),
                    Strictness::Unknown => {
                        Err(self.error(offset, ParseErrorKind::ContextualKeywordRequiresContext))
                    }
                }
            }
            ReservedWordClass::Contextual => {
                if self.context.await_keyword
                    || self.context.await_reserved
                    || self.goal == Goal::Module
                {
                    return Err(self.error(offset, ParseErrorKind::ReservedWord));
                }
                if self.goal == Goal::Unknown {
                    return Err(
                        self.error(offset, ParseErrorKind::ContextualKeywordRequiresContext)
                    );
                }
                // A script's `await` name is invalid if the cover becomes an
                // async arrow head.
                self.cover.await_identifier.get_or_insert(offset);
                Ok(())
            }
            ReservedWordClass::Strict => match self.context.strict {
                Strictness::Sloppy => Ok(()),
                Strictness::Strict => Err(self.error(offset, ParseErrorKind::ReservedWord)),
                Strictness::Unknown => {
                    Err(self.error(offset, ParseErrorKind::ReservedWordRequiresContext))
                }
            },
        }
    }

    // An identifier reference additionally rejects `arguments` in class
    // field initializers and static blocks.
    pub(super) fn check_reference(&mut self, token: IdentifierToken) -> Result<(), ParseError> {
        self.check_name(token)?;
        if self.context.arguments_forbidden
            && identifier_equals(self.slice(token.span), "arguments")
        {
            return Err(self.error(token.span.start(), ParseErrorKind::InvalidArguments));
        }
        Ok(())
    }

    // Strict mode code cannot bind or assign `eval` or `arguments`.
    pub(super) fn check_restricted_name(
        &self,
        span: Span,
        strict: Strictness,
    ) -> Result<(), ParseError> {
        let name = self.slice(span);
        if !(identifier_equals(name, "eval") || identifier_equals(name, "arguments")) {
            return Ok(());
        }
        self.strict_mode_error(span.start(), strict, StrictModeError::RestrictedName)
    }

    // Applies the rules of a binding identifier for code whose strictness was
    // only known after the name was scanned, such as a function name or
    // parameter followed by a `"use strict"` directive.
    pub(super) fn check_binding_strictness(
        &self,
        span: Span,
        strict: Strictness,
    ) -> Result<(), ParseError> {
        self.check_restricted_name(span, strict)?;
        let Some(word) = self.reserved_word_of(span) else {
            return Ok(());
        };
        let strict_only = match word.class() {
            ReservedWordClass::Strict => true,
            ReservedWordClass::Contextual => word == ReservedWord::Yield,
            ReservedWordClass::Always => false,
        };
        if strict_only && strict == Strictness::Strict {
            return Err(self.error(span.start(), ParseErrorKind::ReservedWord));
        }
        Ok(())
    }

    // Reports a construct that strict code forbids, according to `strict`.
    pub(super) fn strict_mode_error(
        &self,
        offset: usize,
        strict: Strictness,
        error: StrictModeError,
    ) -> Result<(), ParseError> {
        match strict {
            Strictness::Sloppy => Ok(()),
            Strictness::Strict => Err(self.error(offset, ParseErrorKind::StrictMode(error))),
            Strictness::Unknown => {
                Err(self.error(offset, ParseErrorKind::StrictModeRequiresContext))
            }
        }
    }

    // Reports whether `token` can begin a binding identifier, as after `let`.
    // Words reserved in every context cannot.
    pub(super) fn can_start_binding(token: IdentifierToken) -> bool {
        token.kind == IdentifierKind::Name
            && token
                .reserved_word
                .is_none_or(|word| word.class() != ReservedWordClass::Always)
    }
}
