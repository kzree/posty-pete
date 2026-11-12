use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub method: HttpMethod,
    pub url: String,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Auth {
    /// Uses the auth from the environment or collection; see [`super::auth`].
    #[default]
    Inherit,
    /// Explicitly no auth.
    None,
    ApiKey {
        key: String,
        value: String,
        #[serde(default, skip_serializing_if = "ApiKeyLocation::is_header")]
        location: ApiKeyLocation,
    },
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
}

/// Where an API key is sent: as a `key: value` header or a `key=value` query param.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

impl Auth {
    pub fn is_inherit(&self) -> bool {
        *self == Self::Inherit
    }

    /// The field values, which can hold `{{name}}` references.
    pub fn texts(&self) -> Vec<&str> {
        match self {
            Self::Inherit | Self::None => Vec::new(),
            Self::ApiKey { key, value, .. } => vec![key, value],
            Self::Basic { username, password } => vec![username, password],
            Self::Bearer { token } => vec![token],
        }
    }

    /// The kind of auth, never its values.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Inherit => "Inherit",
            Self::None => "No auth",
            Self::ApiKey { .. } => "API key",
            Self::Basic { .. } => "Basic auth",
            Self::Bearer { .. } => "Bearer token",
        }
    }
}

impl ApiKeyLocation {
    pub const ALL: [ApiKeyLocation; 2] = [Self::Header, Self::Query];

    pub fn label(self) -> &'static str {
        match self {
            Self::Header => "Header",
            Self::Query => "Query params",
        }
    }

    fn is_header(&self) -> bool {
        *self == Self::Header
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    #[default]
    None,
    /// Sent as `multipart/form-data`.
    FormData(Vec<KeyValue>),
    /// Sent as `application/x-www-form-urlencoded`.
    UrlEncoded(Vec<KeyValue>),
    Raw {
        kind: RawKind,
        text: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RawKind {
    #[default]
    Json,
    Text,
    Xml,
    Html,
}

impl RawKind {
    pub const ALL: [RawKind; 4] = [Self::Json, Self::Text, Self::Xml, Self::Html];

    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Text => "Text",
            Self::Xml => "XML",
            Self::Html => "HTML",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Text => "text/plain; charset=utf-8",
            Self::Xml => "application/xml",
            Self::Html => "text/html",
        }
    }
}

impl Body {
    /// The Content-Type sent unless the request sets one; form data's
    /// multipart type is left to the sender.
    pub fn content_type(&self) -> Option<&'static str> {
        match self {
            Self::None | Self::FormData(_) => None,
            Self::UrlEncoded(_) => Some("application/x-www-form-urlencoded"),
            Self::Raw { kind, .. } => Some(kind.mime()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

    /// The inverse of [`HttpMethod::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|method| method.as_str() == text)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// History stores requests in this format, so a change here breaks
    /// opening older entries. Update it only together with a migration.
    const STORED_FORMAT: &str = r#"[
        {"method": "Get", "url": "example.com", "params": [{"enabled": true, "key": "q", "value": "1"}],
         "headers": [], "body": "None", "auth": "None"},
        {"method": "Post", "url": "", "params": [], "headers": [{"enabled": false, "key": "A", "value": "b"}],
         "body": {"Raw": {"kind": "Json", "text": "{}"}}, "auth": {"ApiKey": {"key": "X-Key", "value": "v"}}},
        {"method": "Post", "url": "", "params": [], "headers": [],
         "body": "None", "auth": {"ApiKey": {"key": "api_key", "value": "v", "location": "Query"}}},
        {"method": "Put", "url": "", "params": [], "headers": [],
         "body": {"Raw": {"kind": "Text", "text": "hi"}}, "auth": {"Basic": {"username": "u", "password": "p"}}},
        {"method": "Patch", "url": "", "params": [], "headers": [],
         "body": {"FormData": [{"enabled": true, "key": "f", "value": "v"}]}, "auth": {"Bearer": {"token": "t"}}},
        {"method": "Patch", "url": "", "params": [], "headers": [],
         "body": {"UrlEncoded": [{"enabled": true, "key": "u", "value": "v"}]}, "auth": "None"},
        {"method": "Patch", "url": "", "params": [], "headers": [],
         "body": {"Raw": {"kind": "Xml", "text": "<a/>"}}, "auth": "None"},
        {"method": "Patch", "url": "", "params": [], "headers": [],
         "body": {"Raw": {"kind": "Html", "text": "<p>"}}, "auth": "None"},
        {"method": "Delete", "url": "", "params": [], "headers": [], "body": "None", "auth": "None"},
        {"method": "Head", "url": "", "params": [], "headers": [], "body": "None", "auth": "None"},
        {"method": "Options", "url": "", "params": [], "headers": [], "body": "None", "auth": "None"},
        {"method": "Options", "url": "", "params": [], "headers": [], "body": "None", "auth": "Inherit"}
    ]"#;

    #[test]
    fn method_parse_inverts_as_str() {
        for method in HttpMethod::ALL {
            assert_eq!(HttpMethod::parse(method.as_str()), Some(method));
        }
        assert_eq!(HttpMethod::parse("get"), None);
        assert_eq!(HttpMethod::parse("BREW"), None);
    }

    #[test]
    fn default_is_empty_get() {
        let request = Request::default();
        assert_eq!(request.method, HttpMethod::Get);
        assert!(request.url.is_empty() && request.params.is_empty() && request.headers.is_empty());
        assert!(matches!(request.body, Body::None));
        assert!(matches!(request.auth, Auth::Inherit));
    }

    #[test]
    fn stored_format_is_stable() {
        let requests: Vec<Request> = serde_json::from_str(STORED_FORMAT).unwrap();
        let mut methods: Vec<_> = requests.iter().map(|r| r.method).collect();
        methods.dedup();
        assert_eq!(methods, HttpMethod::ALL);
        assert!(matches!(
            requests[1].auth,
            Auth::ApiKey {
                location: ApiKeyLocation::Header,
                ..
            }
        ));
        assert!(matches!(
            requests[2].auth,
            Auth::ApiKey {
                location: ApiKeyLocation::Query,
                ..
            }
        ));
        assert!(matches!(requests[0].auth, Auth::None));
        assert!(matches!(requests[11].auth, Auth::Inherit));
        assert!(matches!(&requests[5].body, Body::UrlEncoded(fields) if fields.len() == 1));
        assert!(matches!(
            requests[6].body,
            Body::Raw {
                kind: RawKind::Xml,
                ..
            }
        ));
        assert!(matches!(
            requests[7].body,
            Body::Raw {
                kind: RawKind::Html,
                ..
            }
        ));

        let expected: serde_json::Value = serde_json::from_str(STORED_FORMAT).unwrap();
        assert_eq!(serde_json::to_value(&requests).unwrap(), expected);
    }
}
