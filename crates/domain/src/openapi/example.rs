//! Example values generated from schemas.

use serde_json::{Map, Value};

use super::refs::{MAX_REF_HOPS, Spec, reference};

/// How deep generated objects and arrays nest.
pub(super) const MAX_DEPTH: usize = 8;
/// How many values one generated example holds at most.
pub(super) const MAX_NODES: usize = 2000;
/// How many bytes one example body takes up at most, pretty-printed.
pub(super) const MAX_BODY_BYTES: usize = 1 << 20;

pub(super) struct Example {
    pub value: Value,
    /// Cut short by the node or depth limit.
    pub truncated: bool,
    /// Cut short by [`MAX_BODY_BYTES`].
    pub oversized: bool,
}

/// An example for `schema`, within the limits.
pub(super) fn generate<'a>(spec: &mut Spec<'a>, schema: &'a Value) -> Example {
    let mut builder = Builder {
        spec,
        budget: MAX_NODES,
        bytes: Bytes::new(),
        truncated: false,
        refs: Vec::new(),
    };
    let value = builder.build(schema, 0).unwrap_or(Value::Null);
    Example {
        value,
        truncated: builder.truncated,
        oversized: builder.bytes.exhausted,
    }
}

/// A copy of `value`, leaving out what doesn't fit in [`MAX_BODY_BYTES`].
pub(super) fn bounded(value: &Value) -> Example {
    let mut bytes = Bytes::new();
    let value = bytes.copy(value, 0).unwrap_or(Value::Null);
    Example {
        value,
        truncated: false,
        oversized: bytes.exhausted,
    }
}

/// The example, default or first enum value the schema itself gives.
pub(super) fn given(schema: &Value) -> Option<&Value> {
    schema
        .get("example")
        .or_else(|| schema.get("examples")?.as_array()?.first())
        .or_else(|| schema.get("default"))
        .or_else(|| schema.get("const"))
        .or_else(|| schema.get("enum")?.as_array()?.first())
}

/// The schema's type; for a 3.1 list of types, the first that isn't null.
pub(super) fn schema_type(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(t)) => Some(t),
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .or(types.first().and_then(Value::as_str)),
        _ if schema.get("properties").is_some() => Some("object"),
        _ if schema.get("items").is_some() => Some("array"),
        _ => None,
    }
}

struct Builder<'s, 'a> {
    spec: &'s mut Spec<'a>,
    budget: usize,
    bytes: Bytes,
    truncated: bool,
    /// The `$ref`s being expanded, to stop at cycles.
    refs: Vec<&'a str>,
}

impl<'a> Builder<'_, 'a> {
    /// `None` leaves the value out: a cycle, or past the limits.
    fn build(&mut self, schema: &'a Value, depth: usize) -> Option<Value> {
        if self.budget == 0 {
            self.truncated = true;
            return None;
        }
        if self.bytes.exhausted {
            return None;
        }
        self.budget -= 1;
        let pushed = self.refs.len();
        let mut schema = schema;
        while let Some(target) = reference(schema) {
            if self.refs.contains(&target) || self.refs.len() - pushed >= MAX_REF_HOPS {
                self.refs.truncate(pushed);
                return None;
            }
            self.refs.push(target);
            let Some(found) = self.spec.lookup(target) else {
                self.refs.truncate(pushed);
                return None;
            };
            schema = found;
        }
        let value = self.build_resolved(schema, depth);
        self.refs.truncate(pushed);
        value
    }

    fn build_resolved(&mut self, schema: &'a Value, depth: usize) -> Option<Value> {
        if !schema.is_object() {
            return None;
        }
        if let Some(value) = given(schema) {
            return self.bytes.copy(value, depth);
        }
        if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
            return self.all_of(schema, all, depth);
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(first) = schema
                .get(key)
                .and_then(Value::as_array)
                .and_then(|options| options.first())
            {
                return self.build(first, depth);
            }
        }
        match schema_type(schema) {
            Some("object") => self.object(schema, depth).map(Value::Object),
            Some("array") => {
                if depth >= MAX_DEPTH {
                    self.truncated = true;
                    return None;
                }
                self.bytes.charge(container_size(depth))?;
                let item = schema.get("items").and_then(|items| {
                    self.bytes.charge(entry_size(depth, None))?;
                    self.build(items, depth + 1)
                });
                Some(Value::Array(item.into_iter().collect()))
            }
            Some("string") => self.scalar(Value::String(sample_string(schema).into())),
            Some("integer" | "number") => self.scalar(Value::from(0)),
            Some("boolean") => self.scalar(Value::Bool(true)),
            _ => self.scalar(Value::Null),
        }
    }

    fn scalar(&mut self, value: Value) -> Option<Value> {
        self.bytes.charge(scalar_size(&value))?;
        Some(value)
    }

    fn object(&mut self, schema: &'a Value, depth: usize) -> Option<Map<String, Value>> {
        if depth >= MAX_DEPTH {
            self.truncated = true;
            return None;
        }
        self.bytes.charge(container_size(depth))?;
        let mut map = Map::new();
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
                if self.bytes.charge(entry_size(depth, Some(name))).is_none() {
                    break;
                }
                if let Some(value) = self.build(property, depth + 1) {
                    map.insert(name.clone(), value);
                }
            }
        }
        Some(map)
    }

    /// Merges the parts' objects; if none is an object, the first part's value.
    fn all_of(&mut self, schema: &'a Value, all: &'a [Value], depth: usize) -> Option<Value> {
        let mut merged: Option<Map<String, Value>> = None;
        let mut first = None;
        for part in all {
            match self.build(part, depth) {
                Some(Value::Object(map)) => merged.get_or_insert_default().extend(map),
                Some(other) => {
                    first.get_or_insert(other);
                }
                None => {}
            }
        }
        if schema.get("properties").is_some()
            && let Some(own) = self.object(schema, depth)
        {
            merged.get_or_insert_default().extend(own);
        }
        merged.map(Value::Object).or(first)
    }
}

/// What's left of the byte budget. Sizes are upper bounds on
/// `serde_json::to_string_pretty` output.
struct Bytes {
    left: usize,
    exhausted: bool,
}

impl Bytes {
    fn new() -> Self {
        Self {
            left: MAX_BODY_BYTES,
            exhausted: false,
        }
    }

    /// Once a charge doesn't fit, every later one fails too.
    fn charge(&mut self, size: usize) -> Option<()> {
        if self.exhausted || size > self.left {
            self.left = 0;
            self.exhausted = true;
            return None;
        }
        self.left -= size;
        Some(())
    }

    fn copy(&mut self, value: &Value, depth: usize) -> Option<Value> {
        match value {
            Value::Array(items) => {
                self.charge(container_size(depth))?;
                let mut copied = Vec::new();
                for item in items {
                    let Some(item) = self
                        .charge(entry_size(depth, None))
                        .and_then(|()| self.copy(item, depth + 1))
                    else {
                        break;
                    };
                    copied.push(item);
                }
                Some(Value::Array(copied))
            }
            Value::Object(map) => {
                self.charge(container_size(depth))?;
                let mut copied = Map::new();
                for (key, item) in map {
                    let Some(item) = self
                        .charge(entry_size(depth, Some(key)))
                        .and_then(|()| self.copy(item, depth + 1))
                    else {
                        break;
                    };
                    copied.insert(key.clone(), item);
                }
                Some(Value::Object(copied))
            }
            scalar => {
                self.charge(scalar_size(scalar))?;
                Some(scalar.clone())
            }
        }
    }
}

/// Brackets, and the newline and indent before the closing one.
fn container_size(depth: usize) -> usize {
    3 + 2 * depth
}

/// The comma, newline and indent before an entry, and an object key.
fn entry_size(depth: usize, key: Option<&str>) -> usize {
    2 + 2 * (depth + 1) + key.map_or(0, |key| string_size(key) + 2)
}

fn scalar_size(value: &Value) -> usize {
    match value {
        Value::Null | Value::Bool(true) => 4,
        Value::Bool(false) => 5,
        Value::Number(n) => n.to_string().len(),
        Value::String(s) => string_size(s),
        Value::Array(_) | Value::Object(_) => 0,
    }
}

/// The length of `s` as a JSON string literal, quotes included.
fn string_size(s: &str) -> usize {
    let escaped: usize = s
        .bytes()
        .map(|b| match b {
            b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 2,
            0..=0x1f => 6,
            _ => 1,
        })
        .sum();
    escaped + 2
}

fn sample_string(schema: &Value) -> &'static str {
    match schema.get("format").and_then(Value::as_str) {
        Some("date-time") => "2024-01-01T00:00:00Z",
        Some("date") => "2024-01-01",
        Some("time") => "00:00:00",
        Some("uuid") => "00000000-0000-0000-0000-000000000000",
        Some("email") => "user@example.com",
        Some("uri" | "url" | "iri") => "https://example.com",
        Some("hostname") => "example.com",
        Some("ipv4") => "192.0.2.1",
        Some("ipv6") => "2001:db8::1",
        Some("binary" | "byte") => "",
        _ => "string",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn charged_bytes_cover_the_pretty_output() {
        let value = json!({
            "text": "quote \" slash \\ tab \t nul \u{0} é",
            "numbers": [0, -12, 3.5, 1e300],
            "flags": [true, false, null],
            "empty": {"list": [], "map": {}},
            "nested": [[{"k\n": [1, [2, {"deep": "x"}]]}]]
        });
        let mut bytes = Bytes::new();
        assert_eq!(bytes.copy(&value, 0), Some(value.clone()));
        let charged = MAX_BODY_BYTES - bytes.left;
        let actual = serde_json::to_string_pretty(&value).unwrap().len();
        assert!(charged >= actual, "{charged} < {actual}");
        assert!(charged - actual < 64, "{charged} vs {actual}");
    }
}
