//! YAML graph contracts and differential checks against the JSON parser.

use frameshift_engine::syntax::{
    JsonDialect, JsonLimits, JsonValue, YamlLimits, YamlValue, parse_json, parse_yaml,
};

pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = crate::targets::text(input)? else {
        return Ok(());
    };
    let source = crate::targets::source(text)?;
    let limits = YamlLimits::default();
    let parsed = parse_yaml(&source, limits);
    if parsed != parse_yaml(&source, limits) {
        return Err("YAML parse is not deterministic".into());
    }
    match parsed {
        Err(error) => {
            source
                .slice(error.span)
                .map_err(|_| "YAML error span is invalid")?;
            if parse_json(
                &source,
                JsonDialect::Json,
                JsonLimits {
                    max_depth: 64,
                    ..JsonLimits::default()
                },
            )
            .is_ok()
            {
                return Err("YAML rejected valid JSON".into());
            }
        }
        Ok(stream) => {
            for node in stream.nodes() {
                source
                    .slice(node.span)
                    .map_err(|_| "YAML node span is invalid")?;
                let valid = |id| stream.node(id).is_some();
                match &node.value {
                    YamlValue::Alias(target) if !valid(*target) => {
                        return Err("YAML alias target is invalid".into());
                    }
                    YamlValue::Alias(_) | YamlValue::Scalar { .. } => {}
                    YamlValue::Sequence { elements, .. } => {
                        if !elements.iter().all(|id| valid(*id)) {
                            return Err("YAML sequence reference is invalid".into());
                        }
                    }
                    YamlValue::Mapping { entries, .. } => {
                        if !entries.iter().all(|(k, v)| valid(*k) && valid(*v)) {
                            return Err("YAML mapping reference is invalid".into());
                        }
                    }
                }
            }
            if !stream
                .documents()
                .iter()
                .all(|document| stream.node(document.root).is_some())
            {
                return Err("YAML document root is invalid".into());
            }
            if !stream.nodes().is_empty()
                && parse_yaml(
                    &source,
                    YamlLimits {
                        max_nodes: stream.nodes().len() - 1,
                        ..limits
                    },
                )
                .is_ok()
            {
                return Err("YAML node bound is ineffective".into());
            }
            if let Ok(json) = parse_json(
                &source,
                JsonDialect::Json,
                JsonLimits {
                    max_depth: 64,
                    ..JsonLimits::default()
                },
            ) {
                if stream.documents().len() != 1 {
                    return Err("JSON is not one YAML document".into());
                }
                let mut pending = vec![(json.root(), stream.documents()[0].root)];
                while let Some((j, y)) = pending.pop() {
                    match (json.value(j).unwrap(), &stream.node(y).unwrap().value) {
                        (JsonValue::Null, YamlValue::Scalar { text, .. })
                            if text.as_ref() == "null" => {}
                        (JsonValue::Bool(value), YamlValue::Scalar { text, .. })
                            if text.as_ref() == if value { "true" } else { "false" } => {}
                        (JsonValue::Number(value), YamlValue::Scalar { text, .. })
                            if text.as_ref() == value.text() => {}
                        (JsonValue::String(value), YamlValue::Scalar { text, .. })
                            if text.as_ref() == value => {}
                        (JsonValue::Array(array), YamlValue::Sequence { elements, .. })
                            if array.len() == elements.len() =>
                        {
                            pending.extend(array.iter().zip(elements.iter().copied()))
                        }
                        (JsonValue::Object(object), YamlValue::Mapping { entries, .. })
                            if object.len() == entries.len() =>
                        {
                            for (member, (key, value)) in object.members().zip(entries) {
                                if stream.scalar(*key) != Some(member.key()) {
                                    return Err("JSON and YAML mapping keys differ".into());
                                }
                                pending.push((member.value(), *value));
                            }
                        }
                        _ => return Err("JSON and YAML values differ".into()),
                    }
                }
            }
        }
    }
    Ok(())
}
