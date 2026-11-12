//! Reading spec text into a JSON value, and telling the spec's version.

use serde_json::{Map, Number, Value};

use super::ImportError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Version {
    Swagger2,
    OpenApi3 { minor: u32 },
}

/// Parses JSON, or YAML when the text doesn't start like JSON.
pub(super) fn parse(text: &str) -> Result<Value, ImportError> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim_start().starts_with(['{', '[']) {
        return serde_json::from_str(text).map_err(|err| {
            let line = (err.line() > 0).then_some(err.line());
            parse_error(err.to_string(), line, err.column())
        });
    }
    let mut yaml: serde_norway::Value = serde_norway::from_str(text).map_err(|err| {
        let location = err.location();
        parse_error(
            err.to_string(),
            location.as_ref().map(|location| location.line()),
            location.map_or(0, |location| location.column()),
        )
    })?;
    yaml.apply_merge()
        .map_err(|err| parse_error(err.to_string(), None, 0))?;
    Ok(from_yaml(yaml))
}

fn parse_error(message: String, line: Option<usize>, column: usize) -> ImportError {
    let message = line
        .and_then(|line| message.strip_suffix(&format!(" at line {line} column {column}")))
        .map_or_else(|| message.clone(), str::to_string);
    ImportError::Parse { message, line }
}

fn from_yaml(value: serde_norway::Value) -> Value {
    use serde_norway::Value as Yaml;
    match value {
        Yaml::Null => Value::Null,
        Yaml::Bool(b) => Value::Bool(b),
        Yaml::Number(n) => yaml_number(&n),
        Yaml::String(s) => Value::String(s),
        Yaml::Sequence(items) => Value::Array(items.into_iter().map(from_yaml).collect()),
        Yaml::Mapping(mapping) => {
            let mut map = Map::with_capacity(mapping.len());
            for (key, value) in mapping {
                if let Some(key) = yaml_key(key) {
                    map.insert(key, from_yaml(value));
                }
            }
            Value::Object(map)
        }
        Yaml::Tagged(tagged) => from_yaml(tagged.value),
    }
}

fn yaml_number(n: &serde_norway::Number) -> Value {
    if let Some(i) = n.as_i64() {
        Value::from(i)
    } else if let Some(u) = n.as_u64() {
        Value::from(u)
    } else {
        n.as_f64()
            .and_then(Number::from_f64)
            .map_or_else(|| Value::String(n.to_string()), Value::Number)
    }
}

/// YAML allows any key; specs use strings, and numbers for status codes.
fn yaml_key(key: serde_norway::Value) -> Option<String> {
    use serde_norway::Value as Yaml;
    match key {
        Yaml::String(s) => Some(s),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        Yaml::Null => Some("null".into()),
        Yaml::Tagged(tagged) => yaml_key(tagged.value),
        Yaml::Sequence(_) | Yaml::Mapping(_) => None,
    }
}

pub(super) fn version(root: &Value) -> Result<Version, ImportError> {
    let Some(object) = root.as_object() else {
        return Err(ImportError::NotOpenApi);
    };
    if let Some(openapi) = object.get("openapi") {
        let version = version_text(openapi);
        let minor = version
            .strip_prefix("3.")
            .and_then(|rest| rest.split('.').next())
            .and_then(|minor| minor.parse().ok());
        return match minor {
            Some(minor) => Ok(Version::OpenApi3 { minor }),
            None => Err(ImportError::UnsupportedVersion(format!(
                "OpenAPI {version}"
            ))),
        };
    }
    if let Some(swagger) = object.get("swagger") {
        let version = version_text(swagger);
        return if version == "2.0" {
            Ok(Version::Swagger2)
        } else {
            Err(ImportError::UnsupportedVersion(format!(
                "Swagger {version}"
            )))
        };
    }
    Err(ImportError::NotOpenApi)
}

/// YAML reads an unquoted `3.0` as a number.
fn version_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_string(),
        other => other.to_string(),
    }
}
