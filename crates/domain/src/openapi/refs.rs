//! Local `$ref` resolution.

use serde_json::Value;

use super::warnings::{Counted, Warnings};

/// How many `$ref`s in a row are followed before giving up, which also ends
/// reference cycles.
pub(super) const MAX_REF_HOPS: usize = 32;

pub(super) struct Spec<'a> {
    pub root: &'a Value,
    pub warnings: Warnings,
}

impl<'a> Spec<'a> {
    pub fn new(root: &'a Value) -> Self {
        Self {
            root,
            warnings: Warnings::default(),
        }
    }

    /// `value`, or what its `$ref` chain leads to. `None` for an external,
    /// broken or endless chain, with a warning.
    pub fn resolve(&mut self, value: &'a Value) -> Option<&'a Value> {
        let mut current = value;
        for _ in 0..MAX_REF_HOPS {
            match reference(current) {
                Some(target) => current = self.lookup(target)?,
                None => return Some(current),
            }
        }
        self.warnings
            .count(Counted::BrokenRef, reference(current).unwrap_or_default());
        None
    }

    /// What one `$ref` points to.
    pub fn lookup(&mut self, target: &str) -> Option<&'a Value> {
        let Some(pointer) = target.strip_prefix('#') else {
            self.warnings.count(Counted::ExternalRef, target);
            return None;
        };
        let found = self.root.pointer(&percent_decode(pointer));
        if found.is_none() {
            self.warnings.count(Counted::BrokenRef, target);
        }
        found
    }

    /// `object[key]`, resolved.
    pub fn get(&mut self, object: &'a Value, key: &str) -> Option<&'a Value> {
        self.resolve(object.get(key)?)
    }
}

pub(super) fn reference(value: &Value) -> Option<&str> {
    value.get("$ref")?.as_str()
}

/// JSON pointers in URI fragments may be percent-encoded.
fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoded_pointers_decode() {
        assert_eq!(percent_decode("/paths/~1a%7Bb%7D"), "/paths/~1a{b}");
        assert_eq!(percent_decode("/plain"), "/plain");
        assert_eq!(percent_decode("/bad%zz"), "/bad%zz");
    }
}
