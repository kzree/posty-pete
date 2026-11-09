use std::time::Duration;

use super::body_format::BodyFormat;

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusClass {
    Informational,
    Success,
    Redirect,
    ClientError,
    ServerError,
}

impl Response {
    pub fn status_class(&self) -> StatusClass {
        match self.status {
            200..=299 => StatusClass::Success,
            300..=399 => StatusClass::Redirect,
            400..=499 => StatusClass::ClientError,
            500..=599 => StatusClass::ServerError,
            _ => StatusClass::Informational,
        }
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

    pub fn pretty_body(&self) -> String {
        if self.body_format() != BodyFormat::Json {
            return self.body.clone();
        }
        serde_json::from_str::<serde_json::Value>(&self.body)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| self.body.clone())
    }
}
