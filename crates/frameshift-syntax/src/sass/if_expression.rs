//! Modern CSS-style `if()` and its parse-time condition grammar (Dart Sass 1.105.2).

use super::{
    Parser, SassConditionOperator as Operator, SassError, SassErrorKind as Error, SassNodeId as Id,
    SassNodeKind as Kind, condition::ValueOptions,
};

impl Parser<'_> {
    pub(super) fn if_expression(&mut self, start: usize, name: Id) -> Result<Id, SassError> {
        self.enter()?;
        self.expect("(")?;
        self.if_space()?;
        let before = self.expression_newlines;
        let comparison = self.until_comparison;
        self.expression_newlines = true;
        self.until_comparison = false;
        let mut branches = vec![name];
        while !self.at(")") {
            let branch_start = self.pos;
            let otherwise = self.module_keyword("else", false)?;
            let condition = if otherwise {
                self.node(branch_start, Kind::Identifier, Vec::new())?
            } else {
                self.if_condition()?
            };
            self.if_space()?;
            self.expect(":")?;
            self.if_space()?;
            let value = self.expression(true)?;
            branches.push(self.node(
                branch_start,
                Kind::IfBranch { otherwise },
                vec![condition, value],
            )?);
            self.if_space()?;
            if !self.eat(";")? {
                break;
            }
            self.if_space()?;
        }
        if branches.len() == 1 {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.expect(")")?;
        self.expression_newlines = before;
        self.until_comparison = comparison;
        let result = self.node(start, Kind::IfExpression, branches);
        self.depth -= 1;
        result
    }

    fn if_space(&mut self) -> Result<(), SassError> {
        loop {
            if self.peek().is_some_and(super::css_space) {
                self.bump()?;
            } else if self.at("/*") {
                self.raw_loud_comment()?;
            } else if self.at("//") {
                self.comment(true, false)?;
            } else {
                return Ok(());
            }
        }
    }

    fn check_if_raw_sass(&mut self, parts: &[Id]) -> Result<(), SassError> {
        let mut stack = parts.to_vec();
        while let Some(id) = stack.pop() {
            self.visit()?;
            let node = &self.nodes[id.0];
            if node.kind == Kind::IfConditionSass {
                return Err(self.error(Error::ExpectedSyntax));
            }
            if matches!(
                node.kind,
                Kind::IfConditionParenthesized
                    | Kind::IfConditionNegation
                    | Kind::IfConditionOperation
                    | Kind::IfConditionRaw
            ) {
                stack.extend_from_slice(&node.children);
            }
        }
        Ok(())
    }

    fn if_keyword(&mut self, word: &str, op: Operator) -> Result<Option<Id>, SassError> {
        let start = self.pos;
        if !self.module_keyword(word, false)? {
            return Ok(None);
        }
        if self.at("(") {
            return Err(self.error(Error::ExpectedSyntax));
        }
        let id = self.node(start, Kind::ConditionOperator(op), Vec::new())?;
        self.if_space()?;
        Ok(Some(id))
    }

    fn if_condition(&mut self) -> Result<Id, SassError> {
        self.enter()?;
        let result = self.if_condition_inner();
        self.depth -= 1;
        result
    }

    fn if_condition_inner(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        if let Some(not) = self.if_keyword("not", Operator::Not)? {
            let group = self.if_group()?;
            return self.node(start, Kind::IfConditionNegation, vec![not, group]);
        }
        let first = self.if_group()?;
        let mut parts = vec![first];
        let mut last = first;
        let mut op = None;
        let mut raw = false;
        self.if_space()?;
        loop {
            let mut keyword = None;
            for (word, operator) in [("and", Operator::And), ("or", Operator::Or)] {
                if op.is_none_or(|old| old == operator)
                    && let Some(id) = self.if_keyword(word, operator)?
                {
                    keyword = Some((id, operator));
                    break;
                }
            }
            if let Some((id, operator)) = keyword {
                op.get_or_insert(operator);
                parts.push(id);
                let group = self.if_group()?;
                parts.push(group);
                // The pinned raw-condition reader leaves its previous group
                // unchanged in its `and` branch. Preserve that boundary rule.
                if !raw || operator == Operator::Or {
                    last = group;
                }
            } else if !self.at(")") && !self.at(":") && self.if_substitution(last) {
                last = self.if_group()?;
                parts.push(last);
                raw = true;
            } else if let Some(group) = self.try_if_substitution()? {
                if !raw && !self.if_substitution(last) && !self.if_substitution(group) {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                parts.push(group);
                last = group;
                raw = true;
            } else {
                break;
            }
            self.if_space()?;
        }
        if parts.len() == 1 {
            return Ok(first);
        }
        if raw {
            self.check_if_raw_sass(&parts)?;
        }
        self.node(
            start,
            if raw {
                Kind::IfConditionRaw
            } else {
                Kind::IfConditionOperation
            },
            parts,
        )
    }

    fn if_group(&mut self) -> Result<Id, SassError> {
        self.enter()?;
        let result = self.if_group_inner();
        self.depth -= 1;
        result
    }

    fn if_group_inner(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        if self.eat("(")? {
            self.if_space()?;
            let condition = self.if_condition()?;
            self.if_space()?;
            self.expect(")")?;
            return self.node(start, Kind::IfConditionParenthesized, vec![condition]);
        }
        if self.module_keyword("sass", true)? {
            self.expect("(")?;
            self.if_space()?;
            let expression = self.expression(true)?;
            self.if_space()?;
            self.expect(")")?;
            return self.node(start, Kind::IfConditionSass, vec![expression]);
        }
        let name = self.condition_identifier()?;
        if self.nodes[name.0].kind == Kind::Interpolation && !self.at("(") {
            return self.node(start, Kind::IfConditionRaw, vec![name]);
        }
        if self.plain_name(name).is_some_and(|s| {
            ["and", "or", "not"]
                .iter()
                .any(|word| s.eq_ignore_ascii_case(word))
        }) {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.if_function(start, name, true)
    }

    fn if_function(&mut self, start: usize, name: Id, skip: bool) -> Result<Id, SassError> {
        self.expect("(")?;
        if skip {
            self.if_space()?;
        }
        let arguments = self.declaration_value(ValueOptions {
            allow_empty: true,
            allow_semicolon: true,
            allow_colon: true,
            lines: true,
        })?;
        self.expect(")")?;
        self.node(start, Kind::IfConditionFunction, vec![name, arguments])
    }

    fn if_substitution(&self, id: Id) -> bool {
        let node = &self.nodes[id.0];
        node.kind == Kind::IfConditionRaw
            || (node.kind == Kind::IfConditionFunction
                && self.plain_name(node.children[0]).is_some_and(|name| {
                    ["if", "var", "attr"]
                        .iter()
                        .any(|word| name.eq_ignore_ascii_case(word))
                        || name.starts_with("--")
                }))
    }

    fn try_if_substitution(&mut self) -> Result<Option<Id>, SassError> {
        let start = self.pos;
        if self.at("#") {
            let interpolation = self.interpolation()?;
            return self
                .node(start, Kind::IfConditionRaw, vec![interpolation])
                .map(Some);
        }
        let checkpoint = self.checkpoint();
        let mut found = false;
        for word in ["if", "var", "attr"] {
            if self.module_keyword(word, false)? {
                found = true;
                break;
            }
        }
        let name = if found {
            self.node(start, Kind::Identifier, Vec::new())?
        } else if self.at("--") {
            self.condition_identifier()?
        } else {
            return Ok(None);
        };
        if !self.at("(") {
            self.restore(&checkpoint);
            return Ok(None);
        }
        self.if_function(start, name, false).map(Some)
    }
}
