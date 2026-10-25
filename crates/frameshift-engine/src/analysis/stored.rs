//! Bounded, read-only parsing of untrusted analysis documents.
//!
//! A stored report is evidence, never a plan to execute. Parsing validates
//! accounting and cross-references without reading any referenced source path.

use super::{AnalyzeError, failure, metrics::Metric};
use crate::{document, limits::Limits};
use frameshift_core::{SourcePath, SourceText};
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
    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(n) => out.push_str(n),
            Self::String(s) => {
                out.push_str(document::print(&document::Value::string(s)).trim_end())
            }
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
                    Value::String(k.clone()).write(out, depth + 1);
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
        Ok(Self { value })
    }
    /// The validated document tree.
    pub fn value(&self) -> &Value {
        &self.value
    }
    /// Re-emits canonical JSON. Equal parsed values produce equal bytes.
    pub fn document(&self) -> String {
        self.value.document()
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
    text(summary, "verdict")?;
    let units = array(v, "units")?;
    if units.len() > limits.analysis.max_units {
        return Err(invalid());
    }
    let mut identities = BTreeSet::new();
    let mut total = 0u64;
    let mut weighted = BTreeMap::new();
    let mut counts = BTreeMap::new();
    for unit in units {
        let id = text(unit, "id")?;
        if id.chars().any(char::is_control)
            || !identities.insert(id)
            || !status(text(unit, "status")?)
        {
            return Err(invalid());
        }
        if !matches!(
            text(unit, "kind")?,
            "route"
                | "layout"
                | "provider"
                | "endpoint"
                | "component"
                | "route-file"
                | "module"
                | "stylesheet"
                | "config"
        ) {
            return Err(invalid());
        }
        if let Some(p) = optional_text(unit, "path")? {
            path(p, limits)?
        }
        let weight = number(unit, "weight")?;
        if !matches!(weight, 2 | 3 | 4 | 5 | 8) {
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
    for route in array(v, "routes")? {
        if !identities.contains(text(route, "unit")?) || !status(text(route, "status")?) {
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
