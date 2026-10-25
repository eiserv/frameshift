//! Deterministic projection of assessment evidence into analysis schema v1.

use super::{
    Analysis, RULESET, SCHEMA_VERSION, TARGET_PROFILE, knowledge,
    metrics::{self, Metric},
};
use crate::document::{self, Value as V};
use frameshift_catalog::Status;
use std::collections::BTreeSet;

fn strings<T: AsRef<str>>(values: impl Iterator<Item = T>) -> V {
    V::Array(values.map(|s| V::string(s.as_ref())).collect())
}
fn metric(numerator: u64, denominator: u64) -> V {
    let metric = Metric {
        numerator,
        denominator,
    };
    V::Object(vec![
        ("numerator", V::Number(numerator)),
        ("denominator", V::Number(denominator)),
        ("basisPoints", V::optional(metric.basis_points(), V::Number)),
    ])
}
fn distribution(analysis: &Analysis, weighted: bool) -> V {
    let total = analysis
        .assessment
        .units
        .iter()
        .map(|u| if weighted { u.weight } else { 1 })
        .sum();
    V::Array(
        metrics::STATUSES
            .iter()
            .map(|&status| {
                let units: Vec<_> = analysis
                    .assessment
                    .units
                    .iter()
                    .filter(|u| u.status == status)
                    .collect();
                let numerator = units
                    .iter()
                    .map(|u| if weighted { u.weight } else { 1 })
                    .sum();
                V::Object(vec![
                    ("status", V::string(status.as_str())),
                    ("count", V::count(units.len())),
                    ("metric", metric(numerator, total)),
                ])
            })
            .collect(),
    )
}
impl Analysis {
    /// Renders version 1 of `frameshift.analysis`: original root-relative
    /// coordinates, integer-only metrics, no source snippets or host paths,
    /// no nondeterministic run metadata, and a final LF.
    pub fn document(&self) -> Result<String, super::AnalyzeError> {
        document::print_bounded(&self.value(), self.max_report_bytes)
            .ok_or_else(|| super::limit("analysis report byte limit reached"))
    }
    /// Deterministic project verdict. Unknown routes never become a positive
    /// suitability claim. Every reason is traceable to the finding list.
    pub fn verdict(&self) -> &'static str {
        let routes = &self.assessment.routes;
        let blocked = routes
            .iter()
            .filter(|r| r.status == Status::Unsupported)
            .count();
        let uncertain = routes
            .iter()
            .filter(|r| r.status == Status::Unknown)
            .count();
        let request_blocked = routes
            .iter()
            .filter(|r| {
                self.assessment.occurrences.iter().any(|o| {
                    o.unit.as_ref().is_some_and(|u| r.closure.contains(u))
                        && matches!(
                            o.occurrence.feature.id,
                            "nextjs.api.request"
                                | "nextjs.api.server-action"
                                | "nextjs.segment.dynamic"
                        )
                })
            })
            .count();
        if request_blocked > routes.len() / 2 {
            "Poor candidate for a static migration: most routes depend on request-time features."
        } else if blocked > 0 {
            "Good candidate with manual work: recognized features block the listed routes; review all unknown evidence."
        } else if uncertain > 0 || routes.is_empty() {
            "Suitability is uncertain: static evidence does not establish route readiness."
        } else {
            "Good candidate: no blocked or uncertain routes; review the listed mapping changes."
        }
    }
    pub(crate) fn value(&self) -> V {
        let units = &self.assessment.units;
        let total: u64 = units.iter().map(|u| u.weight).sum();
        let weight = |statuses: &[Status]| {
            units
                .iter()
                .filter(|u| statuses.contains(&u.status))
                .map(|u| u.weight)
                .sum()
        };
        let detected: BTreeSet<_> = self
            .assessment
            .occurrences
            .iter()
            .filter(|o| o.status.is_some())
            .map(|o| o.occurrence.feature.id)
            .collect();
        let readiness = V::Array(
            metrics::STATUSES
                .iter()
                .map(|status| {
                    V::Object(vec![
                        ("status", V::string(status.as_str())),
                        (
                            "count",
                            V::count(
                                self.assessment
                                    .routes
                                    .iter()
                                    .filter(|r| r.status == *status)
                                    .count(),
                            ),
                        ),
                    ])
                })
                .collect(),
        );
        let summary = V::Object(vec![
            ("verdict", V::string(self.verdict())),
            (
                "evidence",
                strings(
                    self.findings
                        .iter()
                        .filter(|(_, f)| f.severity() != frameshift_core::Severity::Info)
                        .map(|(id, _)| id.to_string()),
                ),
            ),
            ("unitCount", V::count(units.len())),
            ("totalWeight", V::Number(total)),
            ("distribution", distribution(self, true)),
            ("unweightedDistribution", distribution(self, false)),
            (
                "analysisCoverage",
                metric(
                    weight(&[Status::Auto, Status::Review, Status::Unsupported]),
                    total,
                ),
            ),
            (
                "automaticFeasibility",
                metric(weight(&[Status::Auto]), total),
            ),
            (
                "feasibilityWithReview",
                metric(weight(&[Status::Auto, Status::Review]), total),
            ),
            // Assessment predicates are not transformation rules. M5 emits
            // no output operations, so transformation rule coverage is zero.
            ("ruleCoverage", metric(0, detected.len() as u64)),
            ("routeReadiness", readiness),
        ]);
        let unit_values = V::Array(
            units
                .iter()
                .map(|u| {
                    V::Object(vec![
                        ("id", V::string(u.id.as_str())),
                        ("kind", V::string(u.kind.as_str())),
                        (
                            "path",
                            V::optional(u.path.as_ref(), |p| V::string(p.as_str())),
                        ),
                        (
                            "ranges",
                            V::Array(
                                u.ranges
                                    .iter()
                                    .map(|s| {
                                        V::Object(vec![
                                            ("start", V::count(s.start())),
                                            ("end", V::count(s.end())),
                                        ])
                                    })
                                    .collect(),
                            ),
                        ),
                        ("client", V::Bool(u.client)),
                        ("weight", V::Number(u.weight)),
                        ("status", V::string(u.status.as_str())),
                        ("strategy", V::string(u.strategy.as_str())),
                        ("outcome", V::string("not-run")),
                        ("verification", V::Array(Vec::new())),
                        (
                            "findings",
                            strings(
                                self.findings
                                    .iter()
                                    .filter(|(_, f)| f.context().unit.as_ref() == Some(&u.id))
                                    .map(|(id, _)| id.to_string()),
                            ),
                        ),
                    ])
                })
                .collect(),
        );
        let routes = V::Array(
            self.assessment
                .routes
                .iter()
                .map(|r| {
                    V::Object(vec![
                        ("pattern", V::string(&r.pattern)),
                        ("unit", V::string(&r.unit)),
                        ("status", V::string(r.status.as_str())),
                        ("closure", strings(r.closure.iter().map(String::as_str))),
                        ("blocker", V::optional(r.blocker.as_ref(), |b| V::string(b))),
                        (
                            "dependencyPath",
                            strings(r.dependency_path.iter().map(|p| p.as_str())),
                        ),
                        ("outputPath", V::Null),
                    ])
                })
                .collect(),
        );
        let files = V::Array(
            self.assessment
                .files
                .iter()
                .map(|f| {
                    V::Object(vec![
                        ("path", V::string(f.path.as_str())),
                        ("class", V::string(f.class.as_str())),
                        ("units", strings(f.units.iter().map(String::as_str))),
                        ("reason", V::string(&f.reason)),
                        ("outputPaths", V::Array(Vec::new())),
                    ])
                })
                .collect(),
        );
        let occurrences = V::Array(
            self.assessment
                .occurrences
                .iter()
                .map(|o| {
                    let location = match &o.occurrence.location {
                        crate::features::Location::Project => {
                            V::Object(vec![("scope", V::string("project"))])
                        }
                        crate::features::Location::File(p) => V::Object(vec![
                            ("scope", V::string("file")),
                            ("path", V::string(p.as_str())),
                        ]),
                        crate::features::Location::Text { path, span } => V::Object(vec![
                            ("scope", V::string("text")),
                            ("path", V::string(path.as_str())),
                            ("start", V::count(span.start())),
                            ("end", V::count(span.end())),
                        ]),
                    };
                    V::Object(vec![
                        ("feature", V::string(o.occurrence.feature.id)),
                        ("unit", V::optional(o.unit.as_ref(), |u| V::string(u))),
                        ("status", V::optional(o.status, |s| V::string(s.as_str()))),
                        ("rule", V::optional(o.rule.as_ref(), |r| V::string(r))),
                        ("reason", V::string(o.reason.message())),
                        ("location", location),
                        ("detail", V::String(format!("{:?}", o.occurrence.detail))),
                    ])
                })
                .collect(),
        );
        let dependencies = V::Array(
            self.dependencies
                .iter()
                .map(|d| {
                    V::Object(vec![
                        ("name", V::string(&d.name)),
                        ("fields", strings(d.fields.iter().map(String::as_str))),
                        ("declared", strings(d.declared.iter().map(String::as_str))),
                        (
                            "resolved",
                            V::optional(d.resolved.as_ref(), |v| V::string(v)),
                        ),
                        ("class", V::string(d.class.as_str())),
                        ("evidence", V::string(&d.evidence)),
                        ("usedIn", strings(d.used_in.iter().map(|p| p.as_str()))),
                    ])
                })
                .collect(),
        );
        let rules: BTreeSet<_> = self
            .assessment
            .occurrences
            .iter()
            .filter_map(|o| o.rule.as_deref())
            .collect();
        let tasks = V::Array(
            self.findings
                .iter()
                .filter(|(_, f)| f.severity() != frameshift_core::Severity::Info)
                .map(|(id, f)| {
                    V::Object(vec![
                        ("finding", V::String(id.to_string())),
                        ("instruction", V::string(f.remediation())),
                    ])
                })
                .collect(),
        );
        V::Object(vec![
            ("kind", V::string("frameshift.analysis")),
            ("schemaVersion", V::Number(SCHEMA_VERSION)),
            (
                "engineVersion",
                V::string(crate::version::info().engine_version),
            ),
            ("ruleset", V::string(RULESET)),
            ("knowledgeBase", V::string(knowledge::VERSION)),
            ("targetProfile", V::string(TARGET_PROFILE)),
            ("weights", V::string(metrics::VERSION)),
            (
                "source",
                V::Object(vec![
                    ("adapter", V::string("nextjs")),
                    (
                        "version",
                        V::optional(self.source_version.as_ref(), |v| V::string(v)),
                    ),
                    (
                        "package",
                        V::string(self.package.as_ref().map_or(".", |p| p.as_str())),
                    ),
                ]),
            ),
            ("target", V::string("astro-preact")),
            (
                "limits",
                V::Array(
                    self.limits
                        .entries()
                        .iter()
                        .map(|entry| {
                            V::Object(vec![
                                ("name", V::string(entry.name)),
                                ("value", V::Number(entry.value)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("summary", summary),
            ("routes", routes),
            ("units", unit_values),
            ("files", files),
            (
                "unrepresentableEntries",
                V::count(self.assessment.unrepresentable),
            ),
            ("dependencies", dependencies),
            ("occurrences", occurrences),
            (
                "config",
                V::Array(
                    self.assessment
                        .occurrences
                        .iter()
                        .filter(|o| o.occurrence.feature.id.starts_with("nextjs.config."))
                        .map(|o| {
                            V::Object(vec![
                                ("feature", V::string(o.occurrence.feature.id)),
                                ("unit", V::optional(o.unit.as_ref(), |u| V::string(u))),
                                ("status", V::optional(o.status, |s| V::string(s.as_str()))),
                                ("reason", V::string(o.reason.message())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "rules",
                V::Array(
                    rules
                        .iter()
                        .map(|r| {
                            V::Object(vec![
                                ("id", V::string(r)),
                                ("version", V::Number(1)),
                                ("phase", V::string("assessment")),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("findings", crate::inspect::findings(&self.findings)),
            ("verification", V::Array(Vec::new())),
            ("tasks", tasks),
            ("helpers", V::Array(Vec::new())),
        ])
    }
}
