//! `@at-root` queries, as Dart Sass 1.105.2 parses and evaluates them
//! (`D-084`).
//!
//! The parse-time grammar reads parenthesized SassScript with an optional
//! colon. Evaluation joins the evaluated texts as `(kind: rules)` and parses
//! them with its query grammar: `with` or `without`, a colon, and one or more
//! identifiers. A query is established without evidence only when its source
//! already spells that text: one plain identifier naming the kind and plain
//! identifiers as rules.

use super::{
    Parser, SassError, SassListSeparator as Separator, SassNodeId as Id, SassNodeKind as Kind,
    selector::{name_start_char, plain_identifier},
};

impl Parser<'_> {
    /// Reads `(expression)` or `(expression: expression)`.
    pub(super) fn at_root_query(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        self.expect("(")?;
        self.skip(true)?;
        let newlines = self.expression_newlines;
        self.expression_newlines = true;
        let result = self.at_root_query_parts();
        self.expression_newlines = newlines;
        let children = result?;
        self.expect(")")?;
        let id = self.node(start, Kind::AtRootQuery, children)?;
        if !self.established_query(id) {
            self.at_root_evidence.push(id);
        }
        Ok(id)
    }

    fn at_root_query_parts(&mut self) -> Result<Vec<Id>, SassError> {
        let mut children = vec![self.expression(true)?];
        if self.eat(":")? {
            self.skip(true)?;
            children.push(self.expression(true)?);
        }
        Ok(children)
    }

    fn established_query(&self, query: Id) -> bool {
        let [kind, rules] = self.nodes[query.0].children[..] else {
            return false;
        };
        let names_kind = self.unquoted_name(kind).is_some_and(|name| {
            name.eq_ignore_ascii_case("with") || name.eq_ignore_ascii_case("without")
        });
        let list = &self.nodes[rules.0];
        let space_list = list.kind
            == Kind::List {
                separator: Separator::Space,
                bracketed: false,
            };
        names_kind
            && if space_list {
                list.children
                    .iter()
                    .all(|&rule| self.unquoted_name(rule).is_some())
            } else {
                self.unquoted_name(rules).is_some()
            }
    }

    /// The plain text of an unquoted identifier without interpolation.
    fn unquoted_name(&self, id: Id) -> Option<String> {
        let node = &self.nodes[id.0];
        // Hash words and `%` are unquoted strings, but not identifiers.
        (node.kind == Kind::String { quoted: false }
            && node.children.is_empty()
            && identifier_start(self.spelling(id)))
        .then(|| plain_identifier(self.spelling(id)))
    }
}

fn identifier_start(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some('-') => chars
            .next()
            .is_some_and(|c| name_start_char(c) || c == '\\' || c == '-'),
        Some(c) => name_start_char(c) || c == '\\',
        None => false,
    }
}
