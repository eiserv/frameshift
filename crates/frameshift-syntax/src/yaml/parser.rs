use std::collections::BTreeMap;

use super::{
    YamlDocument, YamlError, YamlErrorKind as Error, YamlId, YamlLimits, YamlNode,
    YamlScalarStyle as Style, YamlStream, YamlValue,
};
use frameshift_core::Span;

type Result<T> = std::result::Result<T, YamlError>;

pub(super) fn parse(text: &str, limits: YamlLimits) -> Result<YamlStream> {
    let mut restricted = Vec::new();
    for (offset, ch) in text.char_indices() {
        if !matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{7e}' | '\u{85}' | '\u{a0}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
        {
            if ch < '\u{20}' {
                return Err(YamlError {
                    kind: Error::Character,
                    span: range(offset, offset + ch.len_utf8()),
                });
            }
            restricted.push(offset);
        }
    }
    Parser {
        text,
        pos: 0,
        limits,
        stream: YamlStream {
            nodes: Vec::new(),
            documents: Vec::new(),
        },
        anchors: BTreeMap::new(),
        tags: BTreeMap::new(),
        restricted,
        quoted_ranges: Vec::new(),
    }
    .stream()
}

fn range(start: usize, end: usize) -> Span {
    Span::new(start, end).expect("ordered YAML offsets")
}

struct Parser<'s> {
    text: &'s str,
    pos: usize,
    limits: YamlLimits,
    stream: YamlStream,
    anchors: BTreeMap<Box<str>, YamlId>,
    tags: BTreeMap<Box<str>, Box<str>>,
    restricted: Vec<usize>,
    quoted_ranges: Vec<Span>,
}

impl Parser<'_> {
    fn error(&self, kind: Error) -> YamlError {
        YamlError {
            kind,
            span: range(self.pos, self.pos + self.ch().map_or(0, char::len_utf8)),
        }
    }
    fn ch(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }
    fn at(&self, text: &str) -> bool {
        self.text[self.pos..].starts_with(text)
    }
    fn bump(&mut self) -> Option<char> {
        let ch = self.ch()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }
    fn column(&self) -> usize {
        self.text[..self.pos]
            .rfind(['\n', '\r'])
            .map_or(self.pos, |i| self.pos - i - 1)
    }
    fn line_end(&self) -> usize {
        self.text[self.pos..]
            .find(['\n', '\r'])
            .map_or(self.text.len(), |i| self.pos + i)
    }
    fn eol(&self) -> bool {
        matches!(self.ch(), None | Some('\n' | '\r'))
    }
    fn newline(&mut self) -> bool {
        if self.at("\r\n") {
            self.pos += 2;
            true
        } else if matches!(self.ch(), Some('\r' | '\n')) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn spaces(&mut self) {
        while matches!(self.ch(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }
    fn comment(&mut self) {
        if self.at("#") {
            self.pos = self.line_end();
        }
    }
    fn blank(&mut self) {
        loop {
            if self.column() == 0 && self.at("\u{feff}") {
                self.pos += 3;
            }
            self.spaces();
            self.comment();
            if !self.newline() {
                break;
            }
        }
    }
    fn marker(&self, marker: &str) -> bool {
        self.column() == 0
            && self.at(marker)
            && self.text[self.pos + marker.len()..]
                .chars()
                .next()
                .is_none_or(separation)
    }
    fn indicator(&self, ch: char) -> bool {
        self.ch() == Some(ch)
            && self.text[self.pos + 1..]
                .chars()
                .next()
                .is_none_or(separation)
    }
    fn empty(&mut self) -> Result<YamlId> {
        self.add(self.pos, None, None, scalar(String::new(), Style::Plain))
    }
    fn add(
        &mut self,
        start: usize,
        tag: Option<Box<str>>,
        anchor: Option<Box<str>>,
        value: YamlValue,
    ) -> Result<YamlId> {
        if self.stream.nodes.len() >= self.limits.max_nodes {
            return Err(self.error(Error::NodeLimit));
        }
        self.stream
            .nodes
            .try_reserve(1)
            .map_err(|_| self.error(Error::AllocationFailed))?;
        let id = YamlId(self.stream.nodes.len());
        if let Some(name) = &anchor {
            self.anchors.insert(name.clone(), id);
        }
        self.stream.nodes.push(YamlNode {
            span: range(start, self.pos),
            tag,
            anchor,
            value,
        });
        Ok(id)
    }
    fn finish(&mut self, id: YamlId, value: YamlValue) -> YamlId {
        self.stream.nodes[id.0].span = range(self.stream.nodes[id.0].span.start(), self.pos);
        self.stream.nodes[id.0].value = value;
        id
    }
    fn stream(mut self) -> Result<YamlStream> {
        let mut bare_allowed = true;
        loop {
            self.blank();
            if self.pos == self.text.len() {
                break;
            }
            if self.marker("...") {
                self.pos += 3;
                self.spaces();
                self.comment();
                if !self.eol() {
                    return Err(self.error(Error::Syntax));
                }
                bare_allowed = true;
                continue;
            }
            self.anchors.clear();
            self.tags.clear();
            self.tags.insert("!".into(), "!".into());
            self.tags.insert("!!".into(), "tag:yaml.org,2002:".into());
            let mut directives = false;
            let mut version = false;
            let mut declared_tags = BTreeMap::new();
            while self.column() == 0 && self.at("%") {
                directives = true;
                let end = self.line_end();
                let directive = self.text[self.pos..end].split(" #").next().unwrap();
                let fields: Vec<_> = directive.split_whitespace().collect();
                match fields.first().copied() {
                    Some("%YAML") => {
                        if version
                            || fields.len() != 2
                            || !matches!(fields[1], "1.1" | "1.2" | "1.3")
                        {
                            return Err(self.error(Error::Property));
                        }
                        version = true;
                    }
                    Some("%TAG") => {
                        if fields.len() != 3
                            || !valid_handle(fields[1])
                            || declared_tags.insert(fields[1].to_owned(), ()).is_some()
                        {
                            return Err(self.error(Error::Property));
                        }
                        let uri =
                            decode_uri(fields[2]).ok_or_else(|| self.error(Error::Property))?;
                        self.tags.insert(fields[1].into(), uri.into());
                    }
                    Some(_) => {}
                    None => return Err(self.error(Error::Property)),
                }
                self.pos = end;
                self.blank();
            }
            let explicit_start = self.marker("---");
            if (directives || !bare_allowed) && !explicit_start {
                return Err(self.error(Error::Syntax));
            }
            if self.marker("...") {
                return Err(self.error(Error::Syntax));
            }
            if explicit_start {
                self.pos += 3;
                self.spaces();
                self.comment();
                if self.eol() {
                    self.blank();
                } else if self.mapping_colon().is_some() || self.indicator('-') {
                    return Err(self.error(Error::Syntax));
                }
            }
            let root = if self.pos == self.text.len() || self.marker("---") || self.marker("...") {
                self.empty()?
            } else {
                self.node(-1, 0, false, false, false)?
            };
            self.spaces();
            self.comment();
            if !self.eol() && !self.marker("...") && !self.marker("---") {
                return Err(self.error(Error::Syntax));
            }
            self.blank();
            let explicit_end = self.marker("...");
            if explicit_end {
                self.pos += 3;
                self.spaces();
                self.comment();
                if !self.eol() {
                    return Err(self.error(Error::Syntax));
                }
                self.blank();
            }
            self.stream
                .documents
                .try_reserve(1)
                .map_err(|_| self.error(Error::AllocationFailed))?;
            self.stream.documents.push(YamlDocument {
                root,
                explicit_start,
                explicit_end,
            });
            bare_allowed = explicit_end;
            if self.pos < self.text.len()
                && !self.marker("---")
                && !self.marker("...")
                && !explicit_end
            {
                return Err(self.error(Error::Syntax));
            }
        }
        for offset in self.restricted {
            let after = self
                .quoted_ranges
                .partition_point(|span| span.start() <= offset);
            if after == 0 || self.quoted_ranges[after - 1].end() <= offset {
                return Err(YamlError {
                    kind: Error::Character,
                    span: range(
                        offset,
                        offset + self.text[offset..].chars().next().unwrap().len_utf8(),
                    ),
                });
            }
        }
        Ok(self.stream)
    }

    fn node(
        &mut self,
        parent: isize,
        depth: usize,
        flow: bool,
        key: bool,
        indentless: bool,
    ) -> Result<YamlId> {
        self.spaces();
        let start = self.pos;
        if !flow
            && !key
            && !self.indicator('-')
            && !self.indicator('?')
            && self.mapping_colon().is_some()
        {
            self.depth(depth)?;
            let id = self.add(start, None, None, scalar(String::new(), Style::Plain))?;
            let value = self.block_mapping(self.column(), depth + 1)?;
            return Ok(self.finish(id, value));
        }
        let mut tag = None;
        let mut anchor = None;
        loop {
            if self.at("&") {
                if anchor.is_some() {
                    return Err(self.error(Error::Property));
                }
                self.pos += 1;
                anchor = Some(self.anchor_name()?.into());
            } else if self.at("!") {
                if tag.is_some() {
                    return Err(self.error(Error::Property));
                }
                tag = Some(self.tag()?.into());
            } else {
                break;
            }
            if self.pos < self.text.len()
                && !self.ch().is_some_and(separation)
                && !matches!(self.ch(), Some('[' | '{' | ']' | '}' | ','))
            {
                return Err(self.error(Error::Property));
            }
            self.spaces();
            self.comment();
            if self.newline() {
                self.blank();
                if self.pos == self.text.len()
                    || self.marker("---")
                    || self.marker("...")
                    || (!flow
                        && self.column() as isize <= parent
                        && !(indentless && self.column() as isize == parent && self.indicator('-')))
                {
                    return self.add(start, tag, anchor, scalar(String::new(), Style::Plain));
                }
                if !flow && !key && self.mapping_colon().is_some() {
                    break;
                }
            }
        }
        if self.indicator('-')
            && (tag.is_some() || anchor.is_some())
            && !self.text[start..self.pos].contains(['\r', '\n'])
        {
            return Err(self.error(Error::Syntax));
        }
        if self.at("*") && (flow || key || self.mapping_colon().is_none()) {
            if tag.is_some() || anchor.is_some() {
                return Err(self.error(Error::Property));
            }
            self.pos += 1;
            let name = self.anchor_name()?.to_owned();
            let target = self
                .anchors
                .get(name.as_str())
                .copied()
                .ok_or_else(|| self.error(Error::UnknownAlias))?;
            return self.add(start, None, None, YamlValue::Alias(target));
        }
        let id = self.add(start, tag, anchor, scalar(String::new(), Style::Plain))?;
        let value = if !flow
            && !key
            && !self.indicator('-')
            && !self.indicator('?')
            && self.mapping_colon().is_some()
        {
            self.depth(depth)?;
            self.block_mapping(self.column(), depth + 1)?
        } else {
            match self.ch() {
                Some('[') => {
                    self.depth(depth)?;
                    self.flow_sequence(depth + 1, parent)?
                }
                Some('{') => {
                    self.depth(depth)?;
                    self.flow_mapping(depth + 1, parent)?
                }
                Some('\'' | '"') => {
                    let quote = self.ch().unwrap();
                    scalar(
                        self.quoted(quote, parent, key)?,
                        if quote == '\'' {
                            Style::SingleQuoted
                        } else {
                            Style::DoubleQuoted
                        },
                    )
                }
                Some('|' | '>') if !flow && !key => {
                    let folded = self.at(">");
                    scalar(
                        self.block_scalar(parent, folded)?,
                        if folded {
                            Style::Folded
                        } else {
                            Style::Literal
                        },
                    )
                }
                _ if !flow && !key && self.indicator('-') => {
                    self.depth(depth)?;
                    self.block_sequence(self.column(), depth + 1)?
                }
                _ if !flow
                    && !key
                    && (self.indicator('?')
                        || self.indicator(':')
                        || self.mapping_colon().is_some()) =>
                {
                    self.depth(depth)?;
                    self.block_mapping(self.column(), depth + 1)?
                }
                _ => scalar(self.plain(parent, flow, key)?, Style::Plain),
            }
        };
        if self.at("#")
            && self.pos > 0
            && !self.text[..self.pos]
                .chars()
                .next_back()
                .is_some_and(separation)
        {
            return Err(self.error(Error::Syntax));
        }
        Ok(self.finish(id, value))
    }
    fn depth(&self, depth: usize) -> Result<()> {
        if depth >= self.limits.max_depth.min(128) {
            Err(self.error(Error::DepthLimit))
        } else {
            Ok(())
        }
    }
    fn anchor_name(&mut self) -> Result<&str> {
        let start = self.pos;
        while self
            .ch()
            .is_some_and(|ch| !separation(ch) && !matches!(ch, '[' | ']' | '{' | '}' | ','))
        {
            self.bump();
        }
        if self.pos == start {
            return Err(self.error(Error::Property));
        }
        Ok(&self.text[start..self.pos])
    }
    fn tag(&mut self) -> Result<String> {
        let start = self.pos;
        self.pos += 1;
        if self.at("<") {
            self.pos += 1;
            let start = self.pos;
            while self.ch().is_some_and(|ch| ch != '>' && !separation(ch)) {
                self.bump();
            }
            if !self.at(">") || start == self.pos {
                return Err(self.error(Error::Property));
            }
            let text = &self.text[start..self.pos];
            self.pos += 1;
            return decode_uri(text).ok_or_else(|| self.error(Error::Property));
        }
        while self
            .ch()
            .is_some_and(|ch| !separation(ch) && !matches!(ch, '[' | ']' | '{' | '}' | ','))
        {
            self.bump();
        }
        let text = &self.text[start..self.pos];
        if text == "!" {
            return Ok("!".to_owned());
        }
        let (handle, suffix) = text[1..]
            .find('!')
            .map_or(("!", &text[1..]), |i| (&text[..i + 2], &text[i + 2..]));
        if suffix.is_empty() || !valid_handle(handle) {
            return Err(self.error(Error::Property));
        }
        let prefix = self
            .tags
            .get(handle)
            .ok_or_else(|| self.error(Error::Property))?;
        let suffix = decode_uri(suffix).ok_or_else(|| self.error(Error::Property))?;
        Ok(format!("{prefix}{suffix}"))
    }
    // Only inspect one physical line. Multiline explicit keys use `?`.
    fn mapping_colon(&self) -> Option<usize> {
        let mut quote = None;
        let mut flow = 0usize;
        let mut escaped = false;
        let mut property = false;
        for (i, ch) in self.text[self.pos..self.line_end()].char_indices() {
            if property {
                if !separation(ch) {
                    continue;
                }
                property = false;
            }
            if flow == 0
                && quote.is_none()
                && matches!(ch, '&' | '*' | '!')
                && (i == 0 || self.text.as_bytes()[self.pos + i - 1].is_ascii_whitespace())
            {
                property = true;
                continue;
            }
            if let Some(q) = quote {
                if q == '"' && ch == '\\' && !escaped {
                    escaped = true;
                    continue;
                }
                if ch == q && !escaped {
                    quote = None;
                }
                escaped = false;
                continue;
            }
            match ch {
                '\'' | '"'
                    if i == 0
                        || self.text.as_bytes()[self.pos + i - 1].is_ascii_whitespace()
                        || flow > 0 =>
                {
                    quote = Some(ch)
                }
                '[' | '{' => flow += 1,
                ']' | '}' => flow = flow.saturating_sub(1),
                '#' if i == 0 || self.text.as_bytes()[self.pos + i - 1].is_ascii_whitespace() => {
                    break;
                }
                ':' if flow == 0
                    && self.text[self.pos + i + 1..]
                        .chars()
                        .next()
                        .is_none_or(separation) =>
                {
                    return Some(self.pos + i);
                }
                _ => {}
            }
        }
        None
    }
    fn after_block(&mut self) -> Result<()> {
        self.spaces();
        self.comment();
        if !self.eol() && self.column() != 0 {
            return Err(self.error(Error::Syntax));
        }
        self.blank();
        Ok(())
    }
    fn child(
        &mut self,
        indent: usize,
        depth: usize,
        indentless: bool,
        compact: bool,
    ) -> Result<YamlId> {
        let before_space = self.pos;
        self.spaces();
        self.comment();
        let inline = !self.eol();
        if self.eol() {
            self.blank();
            if self.pos == self.text.len()
                || self.marker("---")
                || self.marker("...")
                || self.column() < indent
                || (self.column() == indent && !(indentless && self.indicator('-')))
            {
                return self.empty();
            }
        }
        let collection =
            self.indicator('-') || self.indicator('?') || self.mapping_colon().is_some();
        if inline && collection && (!compact || self.text[before_space..self.pos].contains('\t')) {
            return Err(self.error(Error::Syntax));
        }
        let line_start = self.pos - self.column();
        if collection && self.text[line_start..self.pos].starts_with('\t') {
            return Err(self.error(Error::Syntax));
        }
        self.node(indent as isize, depth, false, false, indentless)
    }
    fn block_sequence(&mut self, indent: usize, depth: usize) -> Result<YamlValue> {
        let mut elements = Vec::new();
        while self.column() == indent && self.indicator('-') {
            self.pos += 1;
            let value = self.child(indent, depth, false, true)?;
            elements.push(value);
            // A nested block already leaves the cursor on its next sibling.
            if self.column() > indent {
                self.after_block()?;
            } else {
                self.blank();
            }
            if self.marker("---") || self.marker("...") {
                break;
            }
        }
        Ok(YamlValue::Sequence {
            elements,
            flow: false,
        })
    }
    fn block_mapping(&mut self, indent: usize, depth: usize) -> Result<YamlValue> {
        let mut entries = Vec::new();
        loop {
            if self.column() != indent
                || self.marker("---")
                || self.marker("...")
                || self.pos == self.text.len()
            {
                break;
            }
            let explicit = self.indicator('?');
            let key = if explicit {
                self.pos += 1;
                self.child(indent, depth, true, true)?
            } else if self.indicator(':') {
                self.empty()?
            } else {
                let Some(colon) = self.mapping_colon() else {
                    break;
                };
                let start = self.pos;
                let key = self.node(indent as isize, depth, false, true, false)?;
                if self.pos > colon || self.text[start..colon].chars().count() > 1024 {
                    return Err(self.error(Error::Syntax));
                }
                self.spaces();
                if self.pos != colon {
                    return Err(self.error(Error::Syntax));
                }
                key
            };
            self.spaces();
            if explicit && (self.eol() || self.at("#")) {
                self.blank();
            }
            let value = if self.indicator(':')
                && (!explicit
                    || self.column() == indent
                    || self.stream.nodes[key.0].span.end() <= self.pos)
            {
                self.pos += 1;
                self.child(indent, depth, true, explicit)?
            } else if explicit {
                self.empty()?
            } else {
                return Err(self.error(Error::Syntax));
            };
            entries.push((key, value));
            if self.column() > indent {
                self.after_block()?;
            } else {
                self.blank();
            }
        }
        Ok(YamlValue::Mapping {
            entries,
            flow: false,
        })
    }
    fn flow_space(&mut self, parent: isize) -> Result<()> {
        if self.at("#")
            && self.pos > 0
            && !self.text[..self.pos]
                .chars()
                .next_back()
                .is_some_and(separation)
        {
            return Err(self.error(Error::Syntax));
        }
        self.spaces();
        self.comment();
        while self.newline() {
            let start = self.pos;
            self.spaces();
            if self.marker("---") || self.marker("...") {
                return Err(self.error(Error::Syntax));
            }
            let spaces = self.text[start..self.pos]
                .bytes()
                .take_while(|b| *b == b' ')
                .count();
            if !self.eol() && !self.at("#") && spaces as isize <= parent {
                return Err(self.error(Error::Syntax));
            }
            self.comment();
        }
        Ok(())
    }
    fn flow_sequence(&mut self, depth: usize, parent: isize) -> Result<YamlValue> {
        self.pos += 1;
        self.flow_space(parent)?;
        let mut elements = Vec::new();
        while !self.at("]") {
            if self.pos == self.text.len() || self.at(",") {
                return Err(self.error(Error::Syntax));
            }
            let explicit = self.indicator('?');
            if explicit {
                self.pos += 1;
                self.flow_space(parent)?;
            }
            let pair_start = self.pos;
            let key = if self.indicator(':') {
                self.empty()?
            } else {
                self.node(parent, depth, true, false, false)?
            };
            self.flow_space(parent)?;
            let value = if self.at(":") {
                if !explicit && self.text[pair_start..self.pos].contains(['\n', '\r']) {
                    return Err(self.error(Error::Syntax));
                }
                self.pos += 1;
                self.flow_space(parent)?;
                let value = if matches!(self.ch(), Some(',' | ']')) {
                    self.empty()?
                } else {
                    self.node(parent, depth, true, false, false)?
                };
                self.depth(depth)?;
                self.add(
                    pair_start,
                    None,
                    None,
                    YamlValue::Mapping {
                        entries: vec![(key, value)],
                        flow: true,
                    },
                )?
            } else if explicit {
                let empty = self.empty()?;
                self.add(
                    pair_start,
                    None,
                    None,
                    YamlValue::Mapping {
                        entries: vec![(key, empty)],
                        flow: true,
                    },
                )?
            } else {
                key
            };
            elements.push(value);
            self.flow_space(parent)?;
            if self.at("]") {
                break;
            }
            if !self.at(",") {
                return Err(self.error(Error::Syntax));
            }
            self.pos += 1;
            self.flow_space(parent)?;
        }
        self.pos += 1;
        Ok(YamlValue::Sequence {
            elements,
            flow: true,
        })
    }
    fn flow_mapping(&mut self, depth: usize, parent: isize) -> Result<YamlValue> {
        self.pos += 1;
        self.flow_space(parent)?;
        let mut entries = Vec::new();
        while !self.at("}") {
            if self.pos == self.text.len() || self.at(",") {
                return Err(self.error(Error::Syntax));
            }
            if self.indicator('?') {
                self.pos += 1;
                self.flow_space(parent)?;
            }
            let key = if self.at(":") {
                self.empty()?
            } else {
                self.node(parent, depth, true, false, false)?
            };
            self.flow_space(parent)?;
            let value = if self.at(":") {
                self.pos += 1;
                self.flow_space(parent)?;
                if matches!(self.ch(), Some(',' | '}')) {
                    self.empty()?
                } else {
                    self.node(parent, depth, true, false, false)?
                }
            } else {
                self.empty()?
            };
            entries.push((key, value));
            self.flow_space(parent)?;
            if self.at("}") {
                break;
            }
            if !self.at(",") {
                return Err(self.error(Error::Syntax));
            }
            self.pos += 1;
            self.flow_space(parent)?;
        }
        self.pos += 1;
        Ok(YamlValue::Mapping {
            entries,
            flow: true,
        })
    }
    fn plain(&mut self, parent: isize, flow: bool, key: bool) -> Result<String> {
        if self.pos == self.text.len()
            || self.eol()
            || self.at("#")
            || self.marker("---")
            || self.marker("...")
            || matches!(self.ch(), Some(',' | ']' | '}'))
            || self.indicator(':')
        {
            return Ok(String::new());
        }
        if matches!(
            self.ch(),
            Some('@' | '`' | '%' | '|' | '>' | '!' | '&' | '*')
        ) || self.indicator('-')
            || self.indicator('?')
            || (flow
                && matches!(self.ch(), Some('-' | '?' | ':'))
                && matches!(
                    self.text[self.pos + 1..].chars().next(),
                    Some(',' | '[' | ']' | '{' | '}')
                ))
        {
            return Err(self.error(Error::Syntax));
        }
        let mut result = String::new();
        loop {
            let start = self.pos;
            while let Some(ch) = self.ch() {
                if matches!(ch, '\n' | '\r') || (flow && matches!(ch, ',' | '[' | ']' | '{' | '}'))
                {
                    break;
                }
                if ch == ':'
                    && (self.text[self.pos + 1..]
                        .chars()
                        .next()
                        .is_none_or(separation)
                        || (flow
                            && matches!(
                                self.text[self.pos + 1..].chars().next(),
                                Some(',' | '[' | ']' | '{' | '}')
                            )))
                {
                    break;
                }
                if ch == '#'
                    && (self.pos == start
                        || matches!(self.text.as_bytes()[self.pos - 1], b' ' | b'\t'))
                {
                    break;
                }
                self.bump();
            }
            result.push_str(self.text[start..self.pos].trim_end_matches([' ', '\t']));
            if !self.eol() || key {
                break;
            }
            let saved = self.pos;
            let mut breaks = 0;
            while self.newline() {
                breaks += 1;
                self.spaces();
                if self.eol() {
                    continue;
                }
                break;
            }
            if self.pos == self.text.len()
                || self.marker("---")
                || self.marker("...")
                || self.at("#")
                || self.at("\t")
                || (!flow && self.column() as isize <= parent)
                || (flow && matches!(self.ch(), Some(',' | ']' | '}')))
            {
                self.pos = saved;
                break;
            }
            if !flow && self.mapping_colon().is_some() {
                self.pos = saved;
                break;
            }
            if breaks == 1 {
                result.push(' ');
            } else {
                for _ in 1..breaks {
                    result.push('\n');
                }
            }
        }
        Ok(result.trim_end_matches([' ', '\t', '\n']).to_owned())
    }
    fn quoted(&mut self, quote: char, parent: isize, key: bool) -> Result<String> {
        let start = self.pos;
        self.pos += 1;
        let mut result = String::new();
        loop {
            let Some(ch) = self.bump() else {
                return Err(self.error(Error::Syntax));
            };
            if ch == quote {
                if quote == '\'' && self.at("'") {
                    self.pos += 1;
                    result.push('\'');
                    continue;
                }
                self.quoted_ranges.push(range(start, self.pos));
                return Ok(result);
            }
            if quote == '"' && ch == '\\' {
                if self.eol() {
                    if key {
                        return Err(self.error(Error::Syntax));
                    }
                    self.folded_break(&mut result, parent, true)?;
                    continue;
                }
                let Some(escape) = self.bump() else {
                    return Err(self.error(Error::Syntax));
                };
                let decoded = match escape {
                    '0' => '\0',
                    'a' => '\u{7}',
                    'b' => '\u{8}',
                    't' | '\t' => '\t',
                    'n' => '\n',
                    'v' => '\u{b}',
                    'f' => '\u{c}',
                    'r' => '\r',
                    'e' => '\u{1b}',
                    ' ' => ' ',
                    '"' => '"',
                    '/' => '/',
                    '\\' => '\\',
                    'N' => '\u{85}',
                    '_' => '\u{a0}',
                    'L' => '\u{2028}',
                    'P' => '\u{2029}',
                    'x' | 'u' | 'U' => {
                        let count = match escape {
                            'x' => 2,
                            'u' => 4,
                            _ => 8,
                        };
                        let mut code = 0u32;
                        for _ in 0..count {
                            let digit = self
                                .bump()
                                .and_then(|c| c.to_digit(16))
                                .ok_or_else(|| self.error(Error::Character))?;
                            code = (code << 4) | digit;
                        }
                        char::from_u32(code).ok_or_else(|| self.error(Error::Character))?
                    }
                    _ => return Err(self.error(Error::Property)),
                };
                result.push(decoded);
            } else if matches!(ch, '\n' | '\r') {
                if key {
                    return Err(self.error(Error::Syntax));
                }
                self.pos -= 1;
                // Decoded escapes are content, even when the decoded value
                // ends in whitespace. Only literal trailing source blanks fold.
                let mut trim = self.pos;
                while trim > 0
                    && matches!(self.text.as_bytes()[trim - 1], b' ' | b'\t')
                    && (trim < 2 || self.text.as_bytes()[trim - 2] != b'\\')
                {
                    trim -= 1;
                    result.pop();
                }
                self.folded_break(&mut result, parent, false)?;
            } else {
                result.push(ch);
            }
        }
    }
    fn folded_break(&mut self, result: &mut String, parent: isize, escaped: bool) -> Result<()> {
        self.newline();
        let start = self.pos;
        self.spaces();
        let mut extra = 0;
        while self.newline() {
            extra += 1;
            self.spaces();
        }
        if self.marker("---")
            || self.marker("...")
            || self.pos == self.text.len()
            || self.column() as isize <= parent
            || (self.text[start..self.pos].starts_with('\t') && parent >= 0)
        {
            return Err(self.error(Error::Syntax));
        }
        if extra == 0 && !escaped {
            result.push(' ');
        } else {
            for _ in 0..extra {
                result.push('\n');
            }
        }
        Ok(())
    }
    fn block_scalar(&mut self, parent: isize, folded: bool) -> Result<String> {
        self.pos += 1;
        let mut chomping = ' ';
        let mut explicit = None;
        for _ in 0..2 {
            match self.ch() {
                Some('+' | '-') if chomping == ' ' => {
                    chomping = self.bump().unwrap();
                }
                Some('1'..='9') if explicit.is_none() => {
                    explicit = Some((self.bump().unwrap() as u8 - b'0') as usize);
                }
                _ => break,
            }
        }
        if !self.eol() && !matches!(self.ch(), Some(' ' | '\t')) {
            return Err(self.error(Error::Syntax));
        }
        self.spaces();
        self.comment();
        if !self.eol() {
            return Err(self.error(Error::Syntax));
        }
        self.newline();
        let base = parent.max(0) as usize;
        let mut indent = explicit.map(|amount| base + amount);
        let mut lines: Vec<(String, bool)> = Vec::new();
        let mut leading_max = 0;
        while self.pos < self.text.len() {
            let start = self.pos;
            let mut spaces = 0;
            while self.at(" ") {
                self.pos += 1;
                spaces += 1;
            }
            let end = self.line_end();
            let blank = self.pos == end;
            if self.at("\t") && spaces as isize <= parent {
                return Err(self.error(Error::Syntax));
            }
            if !blank && (self.marker("---") || self.marker("...") || spaces as isize <= parent) {
                self.pos = start;
                break;
            }
            if !blank && indent.is_none() {
                if leading_max > spaces {
                    return Err(self.error(Error::Syntax));
                }
                indent = Some(spaces.max((parent + 1).max(0) as usize));
            }
            if !blank && spaces < indent.unwrap_or((parent + 1).max(0) as usize) {
                self.pos = start;
                break;
            }
            if blank && indent.is_none() {
                leading_max = leading_max.max(spaces);
            }
            let cut = start + spaces.min(indent.unwrap_or(spaces));
            lines.push((
                self.text[cut..end].to_owned(),
                end < self.text.len() || blank,
            ));
            self.pos = end;
            self.newline();
        }
        let mut result = String::new();
        for (i, (line, terminated)) in lines.iter().enumerate() {
            result.push_str(line);
            if *terminated {
                let next = lines.get(i + 1).map(|line| line.0.as_str());
                if folded
                    && !line.is_empty()
                    && !line.starts_with([' ', '\t'])
                    && next.is_some_and(|next| !next.is_empty() && !next.starts_with([' ', '\t']))
                {
                    result.push(' ');
                } else if folded
                    && !line.is_empty()
                    && !line.starts_with([' ', '\t'])
                    && next == Some("")
                    && lines[i + 1..]
                        .iter()
                        .find(|line| !line.0.is_empty())
                        .is_some_and(|line| !line.0.starts_with([' ', '\t']))
                {
                } else {
                    result.push('\n');
                }
            }
        }
        if chomping != '+' {
            let had_break = result.ends_with('\n');
            while result.ends_with('\n') {
                result.pop();
            }
            if chomping != '-' && had_break && !result.is_empty() {
                result.push('\n');
            }
        }
        Ok(result)
    }
}

fn separation(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r')
}
fn scalar(text: String, style: Style) -> YamlValue {
    YamlValue::Scalar {
        text: text.into(),
        style,
    }
}
fn valid_handle(handle: &str) -> bool {
    matches!(handle, "!" | "!!")
        || (handle.starts_with('!')
            && handle.ends_with('!')
            && handle.len() > 2
            && handle[1..handle.len() - 1]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}
fn decode_uri(text: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chars = text.bytes();
    while let Some(ch) = chars.next() {
        if ch == b'%' {
            let high = (chars.next()? as char).to_digit(16)?;
            let low = (chars.next()? as char).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else if ch.is_ascii_alphanumeric() || b"-#;/?:@&=+$,_.!~*'()[]".contains(&ch) {
            bytes.push(ch);
        } else {
            return None;
        }
    }
    String::from_utf8(bytes).ok()
}
