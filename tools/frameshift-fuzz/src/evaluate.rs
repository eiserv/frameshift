//! Static evaluation of the exports of every tree that parses.

use frameshift_engine::bindings::{ExportName, analyze};
use frameshift_engine::evaluate::{EvaluateLimits, Evaluator, Value};
use frameshift_engine::foundation::{PathLimits, SourcePath, SourceText, TextLimits};
use frameshift_engine::limits::Limits;
use frameshift_engine::syntax::{ParseLimits, parse_js_program};

fn depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Value::Object(object) => {
            1 + object
                .entries
                .iter()
                .map(|(_, v)| depth(v))
                .max()
                .unwrap_or(0)
        }
        Value::Undefined
        | Value::Null
        | Value::Bool(_)
        | Value::Number(_)
        | Value::BigInt(_)
        | Value::String(_)
        | Value::Unknown(_) => 0,
    }
}

/// Checks repetition, independence from evaluation order, the value depth
/// bound, known values without unknown parts, and small work limits.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = crate::targets::text(input)? else {
        return Ok(());
    };
    let source =
        SourceText::new(text.to_owned(), TextLimits::default()).map_err(|e| e.to_string())?;
    let path = SourcePath::new("fuzz.mjs", PathLimits::default()).expect("fictional path");
    let names = [
        ExportName::Default,
        ExportName::Assignment,
        ExportName::Named("a".to_owned()),
    ];
    for goal in crate::targets::GOALS {
        for dialect in crate::targets::DIALECTS {
            let Ok(tree) = parse_js_program(&source, goal, dialect, ParseLimits::default()) else {
                continue;
            };
            let bindings = analyze(&tree).map_err(|e| e.to_string())?;
            for max_work in [16, EvaluateLimits::default().max_work] {
                let limits = EvaluateLimits {
                    max_work,
                    ..EvaluateLimits::default()
                };
                let evaluate = |order: &[ExportName]| -> Result<Vec<Value>, String> {
                    let mut evaluator = Evaluator::detached(Limits::default().modules(), limits);
                    let mut values = Vec::new();
                    for name in order {
                        values.push(
                            evaluator
                                .tree_export(&path, &tree, &bindings, name)
                                .map_err(|e| e.to_string())?,
                        );
                    }
                    Ok(values)
                };
                let first = evaluate(&names)?;
                if first != evaluate(&names)? {
                    return Err("evaluation changed on repetition".to_owned());
                }
                for value in &first {
                    if depth(value) > 128 {
                        return Err("a value exceeds the depth bound".to_owned());
                    }
                    if !value.is_known() && value.unknowns().is_empty() {
                        return Err("unknown values lost their reasons".to_owned());
                    }
                    if value.is_known() && !value.unknowns().is_empty() {
                        return Err("known values disagree with their unknown parts".to_owned());
                    }
                }
                // Each export alone, with the full budget, equals the first pass.
                if max_work == EvaluateLimits::default().max_work {
                    let mut reversed = names.to_vec();
                    reversed.reverse();
                    let mut second = evaluate(&reversed)?;
                    second.reverse();
                    if first != second {
                        return Err("evaluation depends on request order".to_owned());
                    }
                }
            }
        }
    }
    Ok(())
}
