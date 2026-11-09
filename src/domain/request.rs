use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub method: HttpMethod,
    pub url: String,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum Body {
    #[default]
    None,
    /// Sent as `multipart/form-data`.
    FormData(Vec<KeyValue>),
    Raw { kind: RawKind, text: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RawKind {
    #[default]
    Json,
    Text,
}

impl RawKind {
    pub const ALL: [RawKind; 2] = [Self::Json, Self::Text];

    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Text => "Text",
        }
    }

    /// Content-Type sent when the user hasn't set one.
    pub fn mime(self) -> mime::Mime {
        match self {
            Self::Json => mime::APPLICATION_JSON,
            Self::Text => mime::TEXT_PLAIN_UTF_8,
        }
    }
}

/// One row of a key/value table (query params, headers, ...).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KeyValue {
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub const ALL: [HttpMethod; 7] = [
        Self::Get,
        Self::Post,
        Self::Put,
        Self::Patch,
        Self::Delete,
        Self::Head,
        Self::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

impl From<HttpMethod> for reqwest::Method {
    fn from(m: HttpMethod) -> Self {
        match m {
            HttpMethod::Get => Self::GET,
            HttpMethod::Post => Self::POST,
            HttpMethod::Put => Self::PUT,
            HttpMethod::Patch => Self::PATCH,
            HttpMethod::Delete => Self::DELETE,
            HttpMethod::Head => Self::HEAD,
            HttpMethod::Options => Self::OPTIONS,
        }
    }
}
