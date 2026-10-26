//! Bounded, read-only parsing of untrusted analysis documents.
//!
//! A stored report is evidence, never a plan to execute. Parsing validates
//! accounting and cross-references without reading any referenced source path.

use super::{AnalyzeError, failure, metrics::Metric};
use crate::{document, limits::Limits};
use frameshift_core::{SourcePath, SourceText, UnitId, UnitKind};
use frameshift_syntax::{JsonDialect, JsonValue, parse_json};
use std::collections::{BTreeMap, BTreeSet};

/// Owned report data, including unknown additive fields for forward compatibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// JSON null.
    Null,
    /// A boolean.
    Bool(bool),
    /// A validated JSON number in original spelling.
    Number(String),
    /// A decoded string. Escape for its output context.
    String(String),
    /// An ordered array.
    Array(Vec<Value>),
    /// An object with exact decoded keys in byte order.
    Object(BTreeMap<String, Value>),
}
impl Value {
    /// Looks up an exact decoded object member.
    pub fn get(&self, key: &str) -> Option<&Value> {
        if let Self::Object(o) = self {
            o.get(key)
        } else {
            None
        }
    }
    /// Returns a decoded string.
    pub fn text(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }
    /// Returns a nonnegative integer only when its spelling is integral.
    pub fn number(&self) -> Option<u64> {
        if let Self::Number(s) = self {
            s.parse().ok()
        } else {
            None
        }
    }
    /// Returns array members, or an empty slice for another value kind.
    pub fn array(&self) -> &[Value] {
        if let Self::Array(a) = self { a } else { &[] }
    }
    /// Canonical, terminal-safe JSON with a final LF.
    pub fn document(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0);
        s.push('\n');
        s
    }
    /// Re-emits JSON only when its exact byte count, including escaping,
    /// indentation and the final LF, fits `max_bytes`.
    pub fn document_bounded(&self, max_bytes: usize) -> Result<String, AnalyzeError> {
        let bytes = self
            .serialized_size(0)
            .checked_add(1)
            .filter(|&n| n <= max_bytes)
            .ok_or_else(|| super::limit("analysis report byte limit exceeded"))?;
        let mut out = String::new();
        out.try_reserve_exact(bytes).map_err(super::limit)?;
        self.write(&mut out, 0);
        out.push('\n');
        Ok(out)
    }
    fn serialized_size(&self, depth: usize) -> usize {
        match self {
            Self::Null => 4,
            Self::Bool(b) => {
                if *b {
                    4
                } else {
                    5
                }
            }
            Self::Number(n) => n.len(),
            Self::String(s) => document::string_size(s),
            Self::Array(a) => {
                if a.is_empty() {
                    return 2;
                }
                a.iter()
                    .fold(3usize.saturating_add(depth.saturating_mul(2)), |n, v| {
                        n.saturating_add(2)
                            .saturating_add((depth + 1).saturating_mul(2))
                            .saturating_add(v.serialized_size(depth + 1))
                    })
                    .saturating_sub(1)
            }
            Self::Object(o) => {
                if o.is_empty() {
                    return 2;
                }
                o.iter()
                    .fold(
                        3usize.saturating_add(depth.saturating_mul(2)),
                        |n, (key, v)| {
                            n.saturating_add(4)
                                .saturating_add((depth + 1).saturating_mul(2))
                                .saturating_add(document::string_size(key))
                                .saturating_add(v.serialized_size(depth + 1))
                        },
                    )
                    .saturating_sub(1)
            }
        }
    }
    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(n) => out.push_str(n),
            Self::String(s) => document::write_string(out, s),
            Self::Array(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',')
                    }
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth + 1));
                    v.write(out, depth + 1)
                }
                if !a.is_empty() {
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth))
                }
                out.push(']')
            }
            Self::Object(o) => {
                out.push('{');
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',')
                    }
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth + 1));
                    document::write_string(out, k);
                    out.push_str(": ");
                    v.write(out, depth + 1)
                }
                if !o.is_empty() {
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth))
                }
                out.push('}')
            }
        }
    }
}
/// A validated version-1 analysis document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredReport {
    value: Value,
    max_report_bytes: usize,
}
impl StoredReport {
    /// Parses at most `analysis.max-report-bytes`, `json.max-nodes`, and the
    /// smaller of `json.max-depth` and 128 levels. Duplicate keys, unknown
    /// schema versions, invalid accounting, and dangling references fail.
    /// Additional fields are preserved. No source files are opened.
    pub fn parse(bytes: Vec<u8>, limits: &Limits) -> Result<Self, AnalyzeError> {
        let source = SourceText::from_bytes(
            bytes,
            frameshift_core::TextLimits {
                max_source_bytes: limits.analysis.max_report_bytes,
            },
        )
        .map_err(failure)?;
        let mut json = limits.json;
        json.max_depth = json.max_depth.min(128);
        let parsed = parse_json(&source, JsonDialect::Json, json).map_err(|e| {
            if matches!(
                e.kind(),
                frameshift_syntax::JsonErrorKind::DepthLimit { .. }
                    | frameshift_syntax::JsonErrorKind::NodeLimit { .. }
                    | frameshift_syntax::JsonErrorKind::AllocationFailed
            ) {
                super::limit(e)
            } else {
                failure(e)
            }
        })?;
        if parsed.duplicate_keys().next().is_some() {
            return Err(invalid());
        }
        // Reverse arena order makes every child available before its parent.
        let mut nodes: Vec<Option<Value>> = vec![None; parsed.node_count()];
        let ids: Vec<_> = parsed.ids().collect();
        for id in ids.into_iter().rev() {
            let value = match parsed.value(id).ok_or_else(invalid)? {
                JsonValue::Null => Value::Null,
                JsonValue::Bool(b) => Value::Bool(b),
                JsonValue::String(s) => Value::String(s.into()),
                JsonValue::Number(_) => Value::Number(
                    source
                        .slice(parsed.span(id).ok_or_else(invalid)?)
                        .map_err(failure)?
                        .into(),
                ),
                JsonValue::Array(a) => Value::Array(
                    a.iter()
                        .map(|child| nodes[child.index()].take().ok_or_else(invalid))
                        .collect::<Result<_, _>>()?,
                ),
                JsonValue::Object(o) => Value::Object(
                    o.members()
                        .map(|member| {
                            Ok((
                                member.key().to_owned(),
                                nodes[member.value().index()].take().ok_or_else(invalid)?,
                            ))
                        })
                        .collect::<Result<_, AnalyzeError>>()?,
                ),
            };
            nodes[id.index()] = Some(value);
        }
        let value = nodes[parsed.root().index()].take().ok_or_else(invalid)?;
        validate(&value, limits)?;
        Ok(Self {
            value,
            max_report_bytes: limits.analysis.max_report_bytes,
        })
    }
    /// The validated document tree.
    pub fn value(&self) -> &Value {
        &self.value
    }
    /// Re-emits canonical JSON within the byte bound supplied to `parse`.
    /// Equal parsed values produce equal bytes. Expansion beyond the bound
    /// fails before allocating the output.
    pub fn document(&self) -> Result<String, AnalyzeError> {
        self.value.document_bounded(self.max_report_bytes)
    }
    /// Renders a report projection under the same retained byte bound.
    pub fn projection_document(&self, value: &Value) -> Result<String, AnalyzeError> {
        value.document_bounded(self.max_report_bytes)
    }
}
fn invalid() -> AnalyzeError {
    failure("invalid or unsupported analysis report")
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, AnalyzeError> {
    v.get(key).and_then(Value::text).ok_or_else(invalid)
}
fn number(v: &Value, key: &str) -> Result<u64, AnalyzeError> {
    v.get(key).and_then(Value::number).ok_or_else(invalid)
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], AnalyzeError> {
    match v.get(key) {
        Some(Value::Array(a)) => Ok(a),
        _ => Err(invalid()),
    }
}
fn status(s: &str) -> bool {
    matches!(s, "auto" | "review" | "unknown" | "unsupported")
}
fn path(s: &str, limits: &Limits) -> Result<(), AnalyzeError> {
    SourcePath::new(s, limits.path)
        .map(|_| ())
        .map_err(|_| invalid())
}
fn unit_identity(unit: &Value, kind: UnitKind, limits: &Limits) -> Result<String, AnalyzeError> {
    fn decode(encoded: &str) -> Result<String, AnalyzeError> {
        let mut bytes = Vec::with_capacity(encoded.len());
        let mut at = 0;
        while at < encoded.len() {
            let byte = encoded.as_bytes()[at];
            if byte == b'%' {
                let hex = encoded.get(at + 1..at + 3).ok_or_else(invalid)?;
                bytes.push(u8::from_str_radix(hex, 16).map_err(|_| invalid())?);
                at += 3;
            } else {
                bytes.push(byte);
                at += 1;
            }
        }
        String::from_utf8(bytes).map_err(|_| invalid())
    }
    let id = text(unit, "id")?;
    if id.len() > limits.identifier.max_bytes {
        return Err(invalid());
    }
    let (prefix, encoded) = id.split_once(':').ok_or_else(invalid)?;
    if prefix != kind.as_str() {
        return Err(invalid());
    }
    let (subject, export) = encoded
        .split_once('#')
        .map_or((encoded, None), |(s, e)| (s, Some(e)));
    let subject = decode(subject)?;
    let export = export.map(decode).transpose()?;
    let canonical = match kind {
        UnitKind::Route | UnitKind::Endpoint => {
            if export.is_some() {
                return Err(invalid());
            }
            UnitId::for_route(kind, &subject, limits.identifier)
        }
        UnitKind::Config => {
            if export.is_some() {
                return Err(invalid());
            }
            UnitId::for_config(&subject, limits.identifier)
        }
        UnitKind::Layout
        | UnitKind::Provider
        | UnitKind::Component
        | UnitKind::RouteFile
        | UnitKind::Module
        | UnitKind::Stylesheet => {
            if optional_text(unit, "path")? != Some(subject.as_str()) {
                return Err(invalid());
            }
            let path = SourcePath::new(&subject, limits.path).map_err(|_| invalid())?;
            UnitId::for_file(kind, &path, export.as_deref(), limits.identifier)
        }
    }
    .map_err(|_| invalid())?;
    if canonical.as_str() != id {
        return Err(invalid());
    }
    Ok(subject)
}
fn optional_text<'a>(v: &'a Value, key: &str) -> Result<Option<&'a str>, AnalyzeError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        _ => Err(invalid()),
    }
}
fn refs(v: &Value, key: &str, known: &BTreeSet<&str>) -> Result<(), AnalyzeError> {
    for r in array(v, key)? {
        if !r.text().is_some_and(|s| known.contains(s)) {
            return Err(invalid());
        }
    }
    Ok(())
}
fn nullable_text<'a>(v: &'a Value, key: &str) -> Result<Option<&'a str>, AnalyzeError> {
    v.get(key).ok_or_else(invalid)?;
    optional_text(v, key)
}
fn text_array<'a>(v: &'a Value, key: &str) -> Result<Vec<&'a str>, AnalyzeError> {
    array(v, key)?
        .iter()
        .map(|v| v.text().ok_or_else(invalid))
        .collect()
}
fn location(v: &Value, limits: &Limits, positions: bool) -> Result<(), AnalyzeError> {
    match text(v, "scope")? {
        "project" => Ok(()),
        "file" => path(text(v, "path")?, limits),
        "text" => {
            path(text(v, "path")?, limits)?;
            let (start, end) = if positions {
                let position = |key| -> Result<_, AnalyzeError> {
                    let p = v.get(key).ok_or_else(invalid)?;
                    let line = number(p, "line")?;
                    let column = number(p, "column")?;
                    if line == 0 || column == 0 {
                        return Err(invalid());
                    }
                    Ok((number(p, "offset")?, line, column))
                };
                let start = position("start")?;
                let end = position("end")?;
                if (start.1, start.2) > (end.1, end.2) {
                    return Err(invalid());
                }
                (start.0, end.0)
            } else {
                (number(v, "start")?, number(v, "end")?)
            };
            if start > end {
                return Err(invalid());
            }
            Ok(())
        }
        _ => Err(invalid()),
    }
}
fn nested_records(
    v: &Value,
    limits: &Limits,
    units: &BTreeSet<&str>,
    findings: &BTreeSet<&str>,
) -> Result<(), AnalyzeError> {
    let mut names = BTreeSet::new();
    for dependency in array(v, "dependencies")? {
        if !names.insert(text(dependency, "name")?)
            || !matches!(
                text(dependency, "class")?,
                "framework"
                    | "compatible"
                    | "compat"
                    | "compat-unverified"
                    | "incompatible"
                    | "unknown"
            )
        {
            return Err(invalid());
        }
        text_array(dependency, "fields")?;
        text_array(dependency, "declared")?;
        nullable_text(dependency, "resolved")?;
        text(dependency, "evidence")?;
        for p in text_array(dependency, "usedIn")? {
            path(p, limits)?;
        }
    }
    for config in array(v, "config")? {
        if crate::catalog::feature(text(config, "feature")?).is_none()
            || nullable_text(config, "status")?.is_some_and(|s| !status(s))
            || optional_text(config, "unit")?.is_some_and(|id| !units.contains(id))
        {
            return Err(invalid());
        }
        text(config, "reason")?;
    }
    let mut rules = BTreeSet::new();
    for rule in array(v, "rules")? {
        if !rules.insert(text(rule, "id")?)
            || number(rule, "version")? != 1
            || text(rule, "phase")? != "assessment"
        {
            return Err(invalid());
        }
    }
    for record in array(v, "occurrences")?.iter().chain(array(v, "findings")?) {
        if optional_text(record, "rule")?.is_some_and(|id| !rules.contains(id)) {
            return Err(invalid());
        }
    }
    for task in array(v, "tasks")? {
        if !findings.contains(text(task, "finding")?) {
            return Err(invalid());
        }
        text(task, "instruction")?;
    }
    let mut names = BTreeSet::new();
    for limit in array(v, "limits")? {
        if !names.insert(text(limit, "name")?) {
            return Err(invalid());
        }
        number(limit, "value")?;
    }
    // Version 1 intentionally leaves these item schemas open. They remain
    // inert report data, including when future producers add records.
    array(v, "verification")?;
    array(v, "helpers")?;
    Ok(())
}
fn validate(v: &Value, limits: &Limits) -> Result<(), AnalyzeError> {
    if text(v, "kind")? != "frameshift.analysis"
        || number(v, "schemaVersion")? != 1
        || text(v, "target")? != "astro-preact"
        || text(v, "weights")? != "weights-v1"
    {
        return Err(invalid());
    }
    for key in [
        "engineVersion",
        "ruleset",
        "knowledgeBase",
        "targetProfile",
        "weights",
    ] {
        text(v, key)?;
    }
    let source = v.get("source").ok_or_else(invalid)?;
    if text(source, "adapter")? != "nextjs" {
        return Err(invalid());
    }
    nullable_text(source, "version")?;
    number(v, "unrepresentableEntries")?;
    if text(source, "package")? != "." {
        path(text(source, "package")?, limits)?;
    }
    let summary = v.get("summary").ok_or_else(invalid)?;
    let units = array(v, "units")?;
    if units.len() > limits.analysis.max_units {
        return Err(invalid());
    }
    let mut identities = BTreeSet::new();
    let mut total = 0u64;
    let mut weighted = BTreeMap::new();
    let mut counts = BTreeMap::new();
    let mut route_patterns = BTreeMap::new();
    for unit in units {
        let id = text(unit, "id")?;
        if id.chars().any(char::is_control)
            || !identities.insert(id)
            || !status(text(unit, "status")?)
        {
            return Err(invalid());
        }
        let kind = match text(unit, "kind")? {
            "route" => UnitKind::Route,
            "layout" => UnitKind::Layout,
            "provider" => UnitKind::Provider,
            "endpoint" => UnitKind::Endpoint,
            "component" => UnitKind::Component,
            "route-file" => UnitKind::RouteFile,
            "module" => UnitKind::Module,
            "stylesheet" => UnitKind::Stylesheet,
            "config" => UnitKind::Config,
            _ => return Err(invalid()),
        };
        let Some(Value::Bool(client)) = unit.get("client") else {
            return Err(invalid());
        };
        let subject = unit_identity(unit, kind, limits)?;
        if matches!(kind, UnitKind::Route | UnitKind::Endpoint) {
            route_patterns.insert(id, subject);
        }
        if let Some(p) = optional_text(unit, "path")? {
            path(p, limits)?
        }
        let weight = number(unit, "weight")?;
        if weight != super::metrics::weight(kind, *client) {
            return Err(invalid());
        }
        total = total.checked_add(weight).ok_or_else(invalid)?;
        *weighted.entry(text(unit, "status")?).or_insert(0u64) += weight;
        *counts.entry(text(unit, "status")?).or_insert(0u64) += 1;
        if text(unit, "outcome")? != "not-run" {
            return Err(invalid());
        }
        if !matches!(
            text(unit, "strategy")?,
            "astro" | "preact-static" | "island" | "preserve" | "not-applicable"
        ) {
            return Err(invalid());
        }
        let mut previous = 0;
        for r in array(unit, "ranges")? {
            let start = number(r, "start")?;
            let end = number(r, "end")?;
            if start < previous || start > end {
                return Err(invalid());
            }
            previous = end;
        }
    }
    if number(summary, "unitCount")? != units.len() as u64
        || number(summary, "totalWeight")? != total
    {
        return Err(invalid());
    }
    for (key, population, weights) in [
        ("distribution", total, &weighted),
        ("unweightedDistribution", units.len() as u64, &counts),
    ] {
        let entries = array(summary, key)?;
        let mut seen = BTreeSet::new();
        if entries.len() != 4 {
            return Err(invalid());
        }
        for entry in entries {
            let s = text(entry, "status")?;
            if !status(s) || !seen.insert(s) {
                return Err(invalid());
            }
            let m = entry.get("metric").ok_or_else(invalid)?;
            if number(m, "numerator")? != *weights.get(s).unwrap_or(&0)
                || number(m, "denominator")? != population
                || number(entry, "count")? != *counts.get(s).unwrap_or(&0)
            {
                return Err(invalid());
            }
            check_metric(m)?;
        }
    }
    for key in [
        "analysisCoverage",
        "automaticFeasibility",
        "feasibilityWithReview",
        "ruleCoverage",
    ] {
        check_metric(summary.get(key).ok_or_else(invalid)?)?
    }
    for (key, expected) in [
        (
            "analysisCoverage",
            total - weighted.get("unknown").copied().unwrap_or(0),
        ),
        (
            "automaticFeasibility",
            weighted.get("auto").copied().unwrap_or(0),
        ),
        (
            "feasibilityWithReview",
            weighted.get("auto").copied().unwrap_or(0)
                + weighted.get("review").copied().unwrap_or(0),
        ),
    ] {
        let metric = summary.get(key).ok_or_else(invalid)?;
        if number(metric, "numerator")? != expected || number(metric, "denominator")? != total {
            return Err(invalid());
        }
    }
    let unit_status: BTreeMap<_, _> = units
        .iter()
        .map(|u| Ok((text(u, "id")?, text(u, "status")?)))
        .collect::<Result<_, AnalyzeError>>()?;
    let rank = |s: &str| {
        ["auto", "review", "unknown", "unsupported"]
            .iter()
            .position(|x| *x == s)
            .unwrap_or(usize::MAX)
    };
    let mut paths = BTreeSet::new();
    let mut physical_owners = BTreeMap::new();
    for unit in units {
        if text(unit, "kind")? != "config"
            && let Some(p) = optional_text(unit, "path")?
        {
            physical_owners.insert(text(unit, "id")?, p);
        }
    }
    let mut associated = BTreeSet::new();
    for file in array(v, "files")? {
        let p = text(file, "path")?;
        path(p, limits)?;
        if !paths.insert(p) {
            return Err(invalid());
        }
        let class = text(file, "class")?;
        if !matches!(class, "units" | "verbatim" | "out-of-scope" | "ignored") {
            return Err(invalid());
        }
        refs(file, "units", &identities)?;
        if (class == "units") == array(file, "units")?.is_empty() {
            return Err(invalid());
        }
        for id in array(file, "units")? {
            let id = id.text().ok_or_else(invalid)?;
            if let Some(owner) = physical_owners.get(id)
                && (*owner != p || !associated.insert(id))
            {
                return Err(invalid());
            }
        }
    }
    if associated.len() != physical_owners.len() {
        return Err(invalid());
    }
    let mut route_units = BTreeSet::new();
    for route in array(v, "routes")? {
        if !route_units.insert(text(route, "unit")?) {
            return Err(invalid());
        }
        if !identities.contains(text(route, "unit")?) || !status(text(route, "status")?) {
            return Err(invalid());
        }
        if route_patterns.get(text(route, "unit")?).map(String::as_str)
            != Some(text(route, "pattern")?)
        {
            return Err(invalid());
        }
        refs(route, "closure", &identities)?;
        let closure = array(route, "closure")?;
        let mut seen = BTreeSet::new();
        let mut maximum = "auto";
        for id in closure {
            let id = id.text().ok_or_else(invalid)?;
            if !seen.insert(id) {
                return Err(invalid());
            }
            let status = unit_status[id];
            if rank(status) > rank(maximum) {
                maximum = status;
            }
        }
        if !seen.contains(text(route, "unit")?) || text(route, "status")? != maximum {
            return Err(invalid());
        }
        if let Some(blocker) = optional_text(route, "blocker")? {
            if !seen.contains(blocker) || unit_status[blocker] != maximum {
                return Err(invalid());
            }
        } else if matches!(maximum, "unknown" | "unsupported") {
            return Err(invalid());
        }

        if let Some(id) = optional_text(route, "blocker")?
            && !identities.contains(id)
        {
            return Err(invalid());
        }
        for p in array(route, "dependencyPath")? {
            path(p.text().ok_or_else(invalid)?, limits)?
        }
        if !text(route, "pattern")?.starts_with('/') {
            return Err(invalid());
        }
    }
    if route_units.len() != route_patterns.len() {
        return Err(invalid());
    }
    let mut readiness = BTreeMap::new();
    for route in array(v, "routes")? {
        *readiness.entry(text(route, "status")?).or_insert(0u64) += 1;
    }
    let readiness_rows = array(summary, "routeReadiness")?;
    let mut seen = BTreeSet::new();
    if readiness_rows.len() != 4 {
        return Err(invalid());
    }
    for row in readiness_rows {
        let status = text(row, "status")?;
        if !seen.insert(status)
            || !self::status(status)
            || number(row, "count")? != readiness.get(status).copied().unwrap_or(0)
        {
            return Err(invalid());
        }
    }
    let mut finding_ids = BTreeSet::new();
    for finding in array(v, "findings")? {
        let id = text(finding, "id")?;
        if id.len() < 5
            || !id.starts_with('F')
            || !id[1..].bytes().all(|b| b.is_ascii_digit())
            || !finding_ids.insert(id)
        {
            return Err(invalid());
        }
        if let Some(id) = optional_text(finding, "unit")?
            && !identities.contains(id)
        {
            return Err(invalid());
        }
        let code = text(finding, "code")?;
        if code.len() != 6
            || !code.starts_with("FS")
            || !code[2..].bytes().all(|c| c.is_ascii_digit())
        {
            return Err(invalid());
        }
        if !matches!(
            text(finding, "severity")?,
            "error" | "blocker" | "unknown" | "review" | "info"
        ) {
            return Err(invalid());
        }
        for key in ["message", "explanation", "remediation"] {
            text(finding, key)?;
        }
        location(finding.get("location").ok_or_else(invalid)?, limits, true)?;
    }
    for unit in units {
        refs(unit, "findings", &finding_ids)?
    }
    refs(summary, "evidence", &finding_ids)?;
    let occurrences = array(v, "occurrences")?;
    if occurrences.len() > limits.analysis.max_occurrences {
        return Err(invalid());
    }
    for occurrence in occurrences {
        if let Some(s) = nullable_text(occurrence, "status")?
            && !status(s)
        {
            return Err(invalid());
        }
        if let Some(id) = nullable_text(occurrence, "unit")?
            && !identities.contains(id)
        {
            return Err(invalid());
        }
        if crate::catalog::feature(text(occurrence, "feature")?).is_none() {
            return Err(invalid());
        }
        nullable_text(occurrence, "rule")?;
        text(occurrence, "reason")?;
        text(occurrence, "detail")?;
        location(
            occurrence.get("location").ok_or_else(invalid)?,
            limits,
            false,
        )?;
    }
    let request_units: BTreeSet<_> = occurrences
        .iter()
        .filter(|o| {
            o.get("feature")
                .and_then(Value::text)
                .is_some_and(super::report::request_feature)
        })
        .filter_map(|o| o.get("unit").and_then(Value::text))
        .collect();
    let routes = array(v, "routes")?;
    let request_blocked = routes
        .iter()
        .filter(|r| {
            r.get("closure")
                .unwrap()
                .array()
                .iter()
                .any(|u| request_units.contains(u.text().unwrap()))
        })
        .count();
    let verdict = super::report::project_verdict(
        routes.len(),
        readiness.get("unsupported").copied().unwrap_or(0) as usize,
        readiness.get("unknown").copied().unwrap_or(0) as usize,
        request_blocked,
    );
    if text(summary, "verdict")? != verdict {
        return Err(invalid());
    }
    let detected: BTreeSet<_> = occurrences
        .iter()
        .filter(|o| o.get("status").is_some_and(|s| !matches!(s, Value::Null)))
        .map(|o| text(o, "feature"))
        .collect::<Result<_, _>>()?;
    let rule_coverage = summary.get("ruleCoverage").ok_or_else(invalid)?;
    if number(rule_coverage, "numerator")? != 0
        || number(rule_coverage, "denominator")? != detected.len() as u64
    {
        return Err(invalid());
    }
    nested_records(v, limits, &identities, &finding_ids)
}
fn check_metric(v: &Value) -> Result<(), AnalyzeError> {
    let n = number(v, "numerator")?;
    let d = number(v, "denominator")?;
    if n > d {
        return Err(invalid());
    }
    let expected = Metric {
        numerator: n,
        denominator: d,
    }
    .basis_points();
    let actual = match v.get("basisPoints") {
        Some(Value::Null) => None,
        Some(v) => Some(v.number().ok_or_else(invalid)?),
        None => return Err(invalid()),
    };
    if expected != actual {
        return Err(invalid());
    }
    Ok(())
}
