use std::time::Duration;

use encoding_rs::{Encoding, UTF_8};
use mime::Mime;

use super::body_format::BodyFormat;

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub elapsed: Duration,
    body_len: usize,
    binary: bool,
    /// `None` when the body is binary or can't be decoded faithfully.
    text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusClass {
    Informational,
    Success,
    Redirect,
    ClientError,
    ServerError,
}

impl StatusClass {
    pub fn from_status(status: u16) -> Self {
        match status {
            200..=299 => Self::Success,
            300..=399 => Self::Redirect,
            400..=499 => Self::ClientError,
            500..=599 => Self::ServerError,
            _ => Self::Informational,
        }
    }
}

impl Response {
    pub fn new(
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        elapsed: Duration,
    ) -> Self {
        let mut response = Self {
            status,
            headers,
            elapsed,
            body_len: body.len(),
            binary: false,
            text: None,
        };
        response.text = decode_body(&body, response.content_type());
        response.binary = response.text.is_none();
        response
    }

    /// Binary or undecodable bodies are shown as a placeholder, not as text.
    pub fn is_binary(&self) -> bool {
        self.binary
    }

    pub fn status_text(&self) -> String {
        let reason = reqwest::StatusCode::from_u16(self.status)
            .ok()
            .and_then(|code| code.canonical_reason());
        match reason {
            Some(reason) => format!("{} {reason}", self.status),
            None => self.status.to_string(),
        }
    }

    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())
    }

    pub fn body_format(&self) -> BodyFormat {
        BodyFormat::from_content_type(self.content_type())
    }

    /// Moves the body text out for display, so the response doesn't keep a
    /// second copy of a large body.
    pub fn take_display_body(&mut self) -> String {
        let Some(text) = self.text.take() else {
            return format!(
                "<binary or non-UTF-8 body, {} bytes, not shown>",
                self.body_len
            );
        };
        if self.body_format() != BodyFormat::Json {
            return text;
        }
        pretty_json(&text).unwrap_or(text)
    }
}

/// Re-indents valid JSON without parsing values, so numbers and duplicate keys are kept as-is.
fn pretty_json(text: &str) -> Option<String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).ok()?;

    fn newline(out: &mut String, depth: usize) {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    }
    let is_ws = |c: &char| matches!(c, ' ' | '\t' | '\n' | '\r');

    let mut out = String::with_capacity(text.len() + text.len() / 2);
    let mut depth = 0;
    let (mut in_string, mut escaped) = (false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                while chars.next_if(is_ws).is_some() {}
                if let Some(close) = chars.next_if(|&n| n == '}' || n == ']') {
                    out.push(close);
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth -= 1;
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if is_ws(&c) => {}
            c => out.push(c),
        }
    }
    Some(out)
}

/// Decodes with the BOM or declared charset, else strict UTF-8.
fn decode_body(bytes: &[u8], content_type: Option<&str>) -> Option<String> {
    let (encoding, bytes) = match Encoding::for_bom(bytes) {
        Some((encoding, bom_len)) => (encoding, &bytes[bom_len..]),
        None => (declared_charset(content_type)?, bytes),
    };
    let text = encoding.decode_without_bom_handling_and_without_replacement(bytes)?;
    (!text.contains('\0')).then(|| text.into_owned())
}

/// `None` for an unsupported charset; UTF-8 when none is declared.
fn declared_charset(content_type: Option<&str>) -> Option<&'static Encoding> {
    let mime = content_type.and_then(|ct| ct.parse::<Mime>().ok());
    match mime.as_ref().and_then(|m| m.get_param(mime::CHARSET)) {
        Some(label) => Encoding::for_label(label.as_str().as_bytes()),
        None => Some(UTF_8),
    }
}

/// UTF-8 when valid, otherwise each byte as its Latin-1 character.
pub fn header_value_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8], content_type: &str) -> Option<String> {
        decode_body(bytes, Some(content_type))
    }

    #[test]
    fn utf8_body() {
        assert_eq!(
            decode("héllo ✓".as_bytes(), "text/plain; charset=utf-8").as_deref(),
            Some("héllo ✓")
        );
        assert_eq!(
            decode_body("{\"a\":\"ü\"}".as_bytes(), None).as_deref(),
            Some("{\"a\":\"ü\"}")
        );
    }

    #[test]
    fn utf8_bom_is_stripped() {
        assert_eq!(
            decode(b"\xEF\xBB\xBF{}", "application/json").as_deref(),
            Some("{}")
        );
    }

    #[test]
    fn legacy_charsets() {
        assert_eq!(
            decode(b"caf\xE9 \x80", "text/plain; charset=windows-1252").as_deref(),
            Some("café €")
        );
        assert_eq!(
            decode(b"caf\xE9", "text/html; charset=ISO-8859-1").as_deref(),
            Some("café")
        );
        assert_eq!(
            decode(b"\x82\xA0", "text/plain; charset=Shift_JIS").as_deref(),
            Some("あ")
        );
    }

    #[test]
    fn malformed_utf8_is_not_shown_as_text() {
        assert_eq!(decode(b"caf\xE9", "text/plain; charset=utf-8"), None);
        assert_eq!(decode_body(b"caf\xE9", None), None);
    }

    #[test]
    fn binary_and_unsupported_bodies() {
        assert_eq!(decode(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", "image/png"), None);
        assert_eq!(decode(b"a\0b", "application/octet-stream"), None);
        assert_eq!(decode(b"abc", "text/plain; charset=x-made-up"), None);
    }

    #[test]
    fn status_classes() {
        for (status, expected) in [
            (100, StatusClass::Informational),
            (200, StatusClass::Success),
            (299, StatusClass::Success),
            (301, StatusClass::Redirect),
            (404, StatusClass::ClientError),
            (503, StatusClass::ServerError),
            (999, StatusClass::Informational),
        ] {
            assert_eq!(StatusClass::from_status(status), expected, "{status}");
        }
    }

    #[test]
    fn status_text_includes_reason_when_known() {
        let response = |status| Response::new(status, Vec::new(), Vec::new(), Duration::ZERO);
        assert_eq!(response(404).status_text(), "404 Not Found");
        assert_eq!(response(599).status_text(), "599");
    }

    #[test]
    fn display_body_marks_binary() {
        let mut response = Response::new(
            200,
            vec![("content-type".into(), "image/png".into())],
            vec![0x89, b'P', b'N', b'G', 0xFF],
            Duration::ZERO,
        );
        assert_eq!(response.text, None);
        assert!(response.is_binary());
        assert_eq!(
            response.take_display_body(),
            "<binary or non-UTF-8 body, 5 bytes, not shown>"
        );
    }

    #[test]
    fn pretty_json_matches_serde_for_ordinary_json() {
        let input = r#" {"a":[1, 2,{"b":null}],"c":{},"d":[ ],"e":"x, y: {z}","f":true} "#;
        let value: serde_json::Value = serde_json::from_str(input).unwrap();
        assert_eq!(
            pretty_json(input).unwrap(),
            serde_json::to_string_pretty(&value).unwrap()
        );
    }

    #[test]
    fn pretty_json_keeps_number_tokens() {
        let input = r#"{"big":18446744073709551617,"dec":0.10000000000000000000000001,"exp":1.5E+300,"neg":-0e-0}"#;
        assert_eq!(
            pretty_json(input).unwrap(),
            "{\n  \"big\": 18446744073709551617,\n  \"dec\": 0.10000000000000000000000001,\n  \"exp\": 1.5E+300,\n  \"neg\": -0e-0\n}"
        );
    }

    #[test]
    fn pretty_json_keeps_duplicate_keys_and_escapes() {
        assert_eq!(
            pretty_json(r#"{"a":1,"a":2,"s":"q\"\\\u00e9,"}"#).unwrap(),
            "{\n  \"a\": 1,\n  \"a\": 2,\n  \"s\": \"q\\\"\\\\\\u00e9,\"\n}"
        );
    }

    #[test]
    fn malformed_json_is_shown_raw() {
        assert_eq!(pretty_json(r#"{"a":1,}"#), None);
        assert_eq!(pretty_json("{\"a\":1} x"), None);
        let mut response = Response::new(
            200,
            vec![("content-type".into(), "application/json".into())],
            b"{\"a\": [1,".to_vec(),
            Duration::ZERO,
        );
        assert_eq!(response.take_display_body(), "{\"a\": [1,");
        assert_eq!(response.text, None);
        assert!(!response.is_binary());
    }

    #[test]
    fn header_values() {
        assert_eq!(header_value_text(b"text/html"), "text/html");
        assert_eq!(
            header_value_text("attachment; filename=\"ü.txt\"".as_bytes()),
            "attachment; filename=\"ü.txt\""
        );
        assert_eq!(header_value_text(b"caf\xE9"), "café");
    }
}
