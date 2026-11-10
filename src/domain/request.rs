use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Request {
    pub method: HttpMethod,
    pub url: String,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum Auth {
    #[default]
    None,
    /// Sent as a header (`key: value`).
    ApiKey {
        key: String,
        value: String,
    },
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum Body {
    #[default]
    None,
    /// Sent as `multipart/form-data`.
    FormData(Vec<KeyValue>),
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
}

impl RawKind {
    pub const ALL: [RawKind; 2] = [Self::Json, Self::Text];

    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Text => "Text",
        }
    }

    pub fn mime(self) -> mime::Mime {
        match self {
            Self::Json => mime::APPLICATION_JSON,
            Self::Text => mime::TEXT_PLAIN_UTF_8,
        }
    }
}

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
        {"method": "Put", "url": "", "params": [], "headers": [],
         "body": {"Raw": {"kind": "Text", "text": "hi"}}, "auth": {"Basic": {"username": "u", "password": "p"}}},
        {"method": "Patch", "url": "", "params": [], "headers": [],
         "body": {"FormData": [{"enabled": true, "key": "f", "value": "v"}]}, "auth": {"Bearer": {"token": "t"}}},
        {"method": "Delete", "url": "", "params": [], "headers": [], "body": "None", "auth": "None"},
        {"method": "Head", "url": "", "params": [], "headers": [], "body": "None", "auth": "None"},
        {"method": "Options", "url": "", "params": [], "headers": [], "body": "None", "auth": "None"}
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
        assert!(matches!(request.auth, Auth::None));
    }

    #[test]
    fn stored_format_is_stable() {
        let requests: Vec<Request> = serde_json::from_str(STORED_FORMAT).unwrap();
        let methods: Vec<_> = requests.iter().map(|r| r.method).collect();
        assert_eq!(methods, HttpMethod::ALL);

        let expected: serde_json::Value = serde_json::from_str(STORED_FORMAT).unwrap();
        assert_eq!(serde_json::to_value(&requests).unwrap(), expected);
    }
}
