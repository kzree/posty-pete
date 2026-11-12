use std::{
    fmt,
    ops::Deref,
    sync::{Arc, OnceLock},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::variables::references;

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
        text: RawText,
    },
}

/// The text of a raw body. Clones share one copy, so a large body can be
/// snapshotted, compared with an unchanged snapshot and scanned for variables
/// without copying or rescanning it. Stored as a plain string.
#[derive(Clone, Default)]
pub struct RawText(Arc<RawTextInner>);

#[derive(Default)]
struct RawTextInner {
    text: String,
    names: OnceLock<Vec<String>>,
}

impl RawText {
    pub fn as_str(&self) -> &str {
        &self.0.text
    }

    /// Whether `a` and `b` share their text, rather than just being equal.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// The names of the `{{name}}` references, each once, in first-seen order.
    /// Scanned on first use and shared by the clones.
    pub(crate) fn reference_names(&self) -> &[String] {
        self.0.names.get_or_init(|| {
            let mut names: Vec<String> = Vec::new();
            for reference in references(self.as_str()) {
                if !names.iter().any(|name| name == reference.name) {
                    names.push(reference.name.to_string());
                }
            }
            names
        })
    }
}

impl Deref for RawText {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for RawText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<String> for RawText {
    fn from(text: String) -> Self {
        Self(Arc::new(RawTextInner {
            text,
            names: OnceLock::new(),
        }))
    }
}

impl From<&str> for RawText {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

impl PartialEq for RawText {
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other) || self.as_str() == other.as_str()
    }
}

impl Eq for RawText {}

impl PartialEq<str> for RawText {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for RawText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Debug for RawText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl Serialize for RawText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RawText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
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
    fn raw_text_clones_share_and_equal_texts_compare_equal() {
        let text = RawText::from("{\"a\": 1}");
        let clone = text.clone();
        assert!(RawText::ptr_eq(&text, &clone));

        let copy = RawText::from(text.to_string());
        assert!(!RawText::ptr_eq(&text, &copy));
        assert_eq!(text, copy);
        assert_ne!(text, RawText::from("{}"));
        assert_eq!(text, "{\"a\": 1}");
    }

    #[test]
    fn raw_text_is_stored_as_a_string() {
        let text = RawText::from("a\nb");
        let json = serde_json::to_string(&text).unwrap();
        assert_eq!(json, r#""a\nb""#);
        assert_eq!(serde_json::from_str::<RawText>(&json).unwrap(), text);
    }

    #[test]
    fn raw_text_reference_names_are_unique_in_first_seen_order() {
        let text = RawText::from("{{b}} {{a}} {{ b }} {{c}} {{a}}");
        assert_eq!(text.reference_names(), ["b", "a", "c"]);
        let clone = text.clone();
        assert!(std::ptr::eq(
            text.reference_names(),
            clone.reference_names()
        ));
    }

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
