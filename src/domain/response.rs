use std::time::Duration;

use super::body_format::BodyFormat;

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed: Duration,
}

impl Response {
    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())
    }

    pub fn body_format(&self) -> BodyFormat {
        BodyFormat::from_content_type(self.content_type())
    }

    /// The body, pretty-printed if it's JSON.
    pub fn pretty_body(&self) -> String {
        if self.body_format() != BodyFormat::Json {
            return self.body.clone();
        }
        serde_json::from_str::<serde_json::Value>(&self.body)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| self.body.clone())
    }
}
