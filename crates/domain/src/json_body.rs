//! Checking and pretty-printing a raw JSON body that may use `{{name}}`
//! references as values.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

use super::variables::references;

/// Where and why a JSON body doesn't parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub message: String,
    /// 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.line, self.column
        )
    }
}

/// The parse error of `text`, or `None` if it's valid JSON or blank.
pub fn validate(text: &str) -> Option<JsonError> {
    if text.trim().is_empty() {
        return None;
    }
    check(text, &mask_references(text)).err()
}

/// `text` pretty-printed with a 2-space indent. Only whitespace between
/// tokens changes, so strings, numbers and references stay as written.
/// A blank `text` comes back unchanged.
pub fn format(text: &str) -> Result<String, JsonError> {
    if text.trim().is_empty() {
        return Ok(text.to_string());
    }
    let masked = mask_references(text);
    check(text, &masked)?;
    Ok(reindent(text, &masked))
}

fn check(text: &str, masked: &str) -> Result<(), JsonError> {
    serde_json::from_str::<Discard>(masked)
        .map(|_| ())
        .map_err(|err| json_error(text, &err))
}

/// Parses a value the way `serde_json::Value` does, so errors are the same,
/// but keeps nothing. `IgnoredAny` alone takes serde_json's skipping path,
/// which reports trailing commas, out-of-range numbers and deep nesting
/// differently or not at all.
struct Discard;

impl<'de> Deserialize<'de> for Discard {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(DiscardVisitor)
    }
}

struct DiscardVisitor;

impl<'de> Visitor<'de> for DiscardVisitor {
    type Value = Discard;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any valid JSON value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Discard, E> {
        Ok(Discard)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Discard, E> {
        Ok(Discard)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Discard, E> {
        Ok(Discard)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Discard, E> {
        Ok(Discard)
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Discard, E> {
        Ok(Discard)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Discard, E> {
        Ok(Discard)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Discard, A::Error> {
        while seq.next_element::<Discard>()?.is_some() {}
        Ok(Discard)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Discard, A::Error> {
        while map.next_key::<IgnoredAny>()?.is_some() {
            map.next_value::<Discard>()?;
        }
        Ok(Discard)
    }
}

/// Replaces each reference with a number of the same byte length, so the
/// masked text parses where a value would and byte offsets stay put.
fn mask_references(text: &str) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut last = 0;
    for reference in references(text) {
        masked.push_str(&text[last..reference.range.start]);
        masked.push('1');
        masked.extend(std::iter::repeat_n('0', reference.range.len() - 1));
        last = reference.range.end;
    }
    masked.push_str(&text[last..]);
    masked
}

fn json_error(text: &str, err: &serde_json::Error) -> JsonError {
    let full = err.to_string();
    let suffix = format!(" at line {} column {}", err.line(), err.column());
    let message = full.strip_suffix(&suffix).unwrap_or(&full).to_string();
    let line = err.line().max(1);
    // serde_json counts bytes; the editor counts characters.
    let line_text = text.split('\n').nth(line - 1).unwrap_or("");
    let column = line_text
        .char_indices()
        .take_while(|(i, _)| *i < err.column())
        .count()
        .max(1);
    JsonError {
        message,
        line,
        column,
    }
}

/// Walks the structure of the valid `masked` and copies each token from
/// `text`, which has the same byte layout.
fn reindent(text: &str, masked: &str) -> String {
    let bytes = masked.as_bytes();
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            open @ (b'{' | b'[') => {
                let close = if open == b'{' { b'}' } else { b']' };
                let next = skip_whitespace(bytes, i + 1);
                out.push(open as char);
                if bytes[next] == close {
                    out.push(close as char);
                    i = next + 1;
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                    i += 1;
                }
            }
            close @ (b'}' | b']') => {
                depth -= 1;
                newline(&mut out, depth);
                out.push(close as char);
                i += 1;
            }
            b',' => {
                out.push(',');
                newline(&mut out, depth);
                i += 1;
            }
            b':' => {
                out.push_str(": ");
                i += 1;
            }
            b'"' => {
                let end = string_end(bytes, i);
                out.push_str(&text[i..end]);
                i = end;
            }
            _ => {
                let end = scalar_end(bytes, i);
                out.push_str(&text[i..end]);
                i = end;
            }
        }
    }
    out
}

fn skip_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n(' ', depth * 2));
}

/// The end of the string starting at `start`, past its closing quote.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    loop {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
}

fn scalar_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b',' | b':' | b']' | b'}'))
        .map_or(bytes.len(), |n| start + n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_at(text: &str) -> (String, usize, usize) {
        let error = validate(text).expect("should be invalid");
        (error.message, error.line, error.column)
    }

    #[test]
    fn blank_and_valid_bodies_have_no_error() {
        for text in [
            "",
            "  \n\t",
            "{}",
            "[1, 2]",
            "\"x\"",
            "null",
            "{\"a\": {\"b\": []}}",
        ] {
            assert_eq!(validate(text), None, "{text:?}");
        }
    }

    #[test]
    fn reports_message_line_and_column() {
        let text = "{\n  \"a\": 1,\n  \"b\": 2\n  \"c\": 3\n}";
        assert_eq!(error_at(text), ("expected `,` or `}`".into(), 4, 3));
        assert_eq!(
            validate(text).unwrap().to_string(),
            "expected `,` or `}` at line 4, column 3"
        );
    }

    #[test]
    fn reports_trailing_commas_and_truncation() {
        assert_eq!(error_at("[1, 2,]"), ("trailing comma".into(), 1, 7));
        assert_eq!(
            error_at("{\"a\": 1"),
            ("EOF while parsing an object".into(), 1, 7)
        );
        assert_eq!(error_at("{} {}"), ("trailing characters".into(), 1, 4));
    }

    #[test]
    fn column_counts_characters_not_bytes() {
        assert_eq!(
            error_at("{\"ünïcödé\": 1 \"x\"}"),
            ("expected `,` or `}`".into(), 1, 15)
        );
    }

    #[test]
    fn bare_and_quoted_references_are_valid() {
        for text in [
            "{\"id\": {{id}}}",
            "{\"id\": \"{{id}}\"}",
            "{\"{{key}}\": [{{ a }}, -{{b}}, \"x{{c}}y\"]}",
            "{{body}}",
        ] {
            assert_eq!(validate(text), None, "{text:?}");
        }
    }

    #[test]
    fn errors_after_references_keep_their_position() {
        assert_eq!(
            error_at("{\"ä\": {{ünï}} \"x\": 1}"),
            ("expected `,` or `}`".into(), 1, 15)
        );
        assert_eq!(
            error_at("{\"a\": {{a}},\n \"b\" {{b}}}"),
            ("expected `:`".into(), 2, 6)
        );
    }

    #[test]
    fn references_where_a_key_belongs_are_flagged() {
        assert_eq!(
            error_at("{{{key}}: 1}"),
            ("key must be a string".into(), 1, 2)
        );
    }

    #[test]
    fn errors_match_parsing_into_a_value() {
        let deep_ok = format!("{}{}", "[".repeat(100), "]".repeat(100));
        let too_deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        let texts = [
            "{}",
            "[1, 2,]",
            "[[1,]]",
            "{\"a\": 1,}",
            "{\"a\": {\"b\": 1,}}",
            "{\"a\" 1}",
            "{1: 2}",
            "[1 2]",
            "[01]",
            "[1.]",
            "[1e]",
            "[-]",
            "[1e400]",
            "[-1e400]",
            "[18446744073709551616]",
            "[tru]",
            "[nul",
            "\"\\x\"",
            "\"\\ud800\"",
            "\"a\u{1}b\"",
            "{\"\\q\": 1}",
            "[\"ünï\" \"x\"]",
            "{\"a\": {{a}}, \"b\": [{{b}},]}",
            "{\"x\": \"{{y}}\" ,, }",
            "{} {}",
            "[",
            "{",
            "\"abc",
            "]",
            &deep_ok,
            &too_deep,
        ];
        for text in texts {
            let masked = mask_references(text);
            let expected = serde_json::from_str::<serde_json::Value>(&masked)
                .err()
                .map(|err| json_error(text, &err));
            assert_eq!(validate(text), expected, "{text:?}");
        }
    }

    #[test]
    fn formats_like_serde_json_pretty() {
        let text = r#"{"a":[1,2.50,{"b":null}],"c":{},"d":[ ],"e":"x, y: {z}"}"#;
        let expected =
            serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(text).unwrap())
                .unwrap()
                .replace("2.5", "2.50");
        assert_eq!(format(text).unwrap(), expected);
    }

    #[test]
    fn formatting_keeps_tokens_as_written() {
        let text = "{\"z\":1,\"a\":\"\\u00e9\\\"\\/\",\"z\":1e5}";
        assert_eq!(
            format(text).unwrap(),
            "{\n  \"z\": 1,\n  \"a\": \"\\u00e9\\\"\\/\",\n  \"z\": 1e5\n}"
        );
    }

    #[test]
    fn formatting_keeps_references() {
        let text =
            "{\"id\":{{id}},\"name\":\"{{ name }}\",\"tags\":[{{a}},\"{{b}}\"],\"{{k}}\":-{{n}}}";
        assert_eq!(
            format(text).unwrap(),
            "{\n  \"id\": {{id}},\n  \"name\": \"{{ name }}\",\n  \"tags\": [\n    {{a}},\n    \"{{b}}\"\n  ],\n  \"{{k}}\": -{{n}}\n}"
        );
    }

    #[test]
    fn formatting_reindents_formatted_text() {
        let text = "{\n    \"a\": [\n        1\n    ]\n}\n";
        let formatted = format(text).unwrap();
        assert_eq!(formatted, "{\n  \"a\": [\n    1\n  ]\n}");
        assert_eq!(format(&formatted).unwrap(), formatted);
    }

    #[test]
    fn formatting_invalid_json_fails_with_the_error() {
        assert_eq!(format("[1 2]"), Err(validate("[1 2]").unwrap()));
    }

    #[test]
    fn formatting_blank_text_leaves_it() {
        assert_eq!(format("  \n").unwrap(), "  \n");
    }
}
