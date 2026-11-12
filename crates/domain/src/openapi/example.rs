//! Example values generated from schemas.

use serde_json::{Map, Value};

use super::refs::{MAX_REF_HOPS, Spec, reference};

/// How deep generated objects and arrays nest.
pub(super) const MAX_DEPTH: usize = 8;
/// How many values one generated example holds at most.
pub(super) const MAX_NODES: usize = 2000;

/// An example for `schema`, and whether it was cut short by the limits.
pub(super) fn generate<'a>(spec: &mut Spec<'a>, schema: &'a Value) -> (Value, bool) {
    let mut builder = Builder {
        spec,
        budget: MAX_NODES,
        truncated: false,
        refs: Vec::new(),
    };
    let value = builder.build(schema, 0).unwrap_or(Value::Null);
    (value, builder.truncated)
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
            return Some(value.clone());
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
                let item = schema
                    .get("items")
                    .and_then(|items| self.build(items, depth + 1));
                Some(Value::Array(item.into_iter().collect()))
            }
            Some("string") => Some(Value::String(sample_string(schema).into())),
            Some("integer" | "number") => Some(Value::from(0)),
            Some("boolean") => Some(Value::Bool(true)),
            _ => Some(Value::Null),
        }
    }

    fn object(&mut self, schema: &'a Value, depth: usize) -> Option<Map<String, Value>> {
        if depth >= MAX_DEPTH {
            self.truncated = true;
            return None;
        }
        let mut map = Map::new();
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
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
