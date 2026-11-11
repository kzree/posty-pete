use std::borrow::Cow;

use super::{
    request::{ApiKeyLocation, Auth, Body, HttpMethod, KeyValue, Request},
    url_scheme::with_default_scheme,
};

/// A request with the editor's rules applied: what goes on the wire, before
/// it is turned into a reqwest request or a curl command.
#[derive(Debug, PartialEq)]
pub struct ResolvedRequest<'a> {
    pub method: HttpMethod,
    /// With a scheme added if it had none; `query` is appended to it.
    pub url: Cow<'a, str>,
    pub query: Vec<(&'a str, &'a str)>,
    /// Including headers derived from auth and body, in send order.
    pub headers: Vec<(&'a str, Cow<'a, str>)>,
    pub basic_auth: Option<(&'a str, &'a str)>,
    pub body: ResolvedBody<'a>,
}

#[derive(Debug, PartialEq)]
pub enum ResolvedBody<'a> {
    None,
    /// Sent as `multipart/form-data`; the sender sets the Content-Type.
    FormData(Vec<(&'a str, &'a str)>),
    UrlEncoded(Vec<(&'a str, &'a str)>),
    Raw(&'a str),
}

impl Request {
    pub fn resolve(&self) -> ResolvedRequest<'_> {
        let mut headers: Vec<(&str, Cow<str>)> = self
            .headers
            .iter()
            .filter(|header| self.sends_header(header))
            .map(|header| (header.key.as_str(), Cow::Borrowed(header.value.as_str())))
            .collect();
        let sends_auth = self.sends_auth();

        let mut query: Vec<(&str, &str)> = active(&self.params).collect();
        if let Auth::ApiKey {
            key,
            value,
            location,
        } = &self.auth
            && sends_auth
        {
            match location {
                ApiKeyLocation::Header => headers.push((key, Cow::Borrowed(value))),
                ApiKeyLocation::Query => query.push((key, value)),
            }
        }
        if let Some(content_type) = self.body.content_type()
            && !headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        {
            headers.push(("Content-Type", Cow::Borrowed(content_type)));
        }

        let mut basic_auth = None;
        match &self.auth {
            Auth::Basic { username, password } if sends_auth => {
                basic_auth = Some((username.as_str(), password.as_str()));
            }
            Auth::Bearer { token } if sends_auth => {
                headers.push(("Authorization", Cow::Owned(format!("Bearer {token}"))));
            }
            _ => {}
        }

        ResolvedRequest {
            method: self.method,
            url: with_default_scheme(&self.url),
            query,
            headers,
            basic_auth,
            body: match &self.body {
                Body::None => ResolvedBody::None,
                Body::FormData(fields) => ResolvedBody::FormData(active(fields).collect()),
                Body::UrlEncoded(fields) => ResolvedBody::UrlEncoded(active(fields).collect()),
                Body::Raw { text, .. } => ResolvedBody::Raw(text),
            },
        }
    }

    /// Whether `resolve` sends `header`, one of this request's headers.
    pub(super) fn sends_header(&self, header: &KeyValue) -> bool {
        // The multipart Content-Type needs the boundary the sender picks.
        is_active(header)
            && !(matches!(self.body, Body::FormData(_))
                && header.key.eq_ignore_ascii_case("content-type"))
    }

    /// Whether `resolve` sends anything from the auth tab. A header (or, for
    /// an API key in the query, a param) the user set takes precedence over it.
    pub(super) fn sends_auth(&self) -> bool {
        let has_header = |name: &str| {
            self.headers
                .iter()
                .any(|header| self.sends_header(header) && header.key.eq_ignore_ascii_case(name))
        };
        match &self.auth {
            Auth::None => false,
            Auth::ApiKey { key, location, .. } => {
                !key.is_empty()
                    && match location {
                        ApiKeyLocation::Header => !has_header(key),
                        ApiKeyLocation::Query => !self
                            .params
                            .iter()
                            .any(|param| is_active(param) && param.key == *key),
                    }
            }
            Auth::Basic { username, password } => {
                !has_header("authorization") && !(username.is_empty() && password.is_empty())
            }
            Auth::Bearer { token } => !has_header("authorization") && !token.is_empty(),
        }
    }
}

/// Whether a param, header or form field row is sent.
pub(super) fn is_active(row: &KeyValue) -> bool {
    row.enabled && !row.key.is_empty()
}

fn active(items: &[KeyValue]) -> impl Iterator<Item = (&str, &str)> {
    items
        .iter()
        .filter(|kv| is_active(kv))
        .map(|kv| (kv.key.as_str(), kv.value.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::RawKind;

    fn kv(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }

    fn disabled(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: false,
            ..kv(key, value)
        }
    }

    fn get(url: &str) -> Request {
        Request {
            method: HttpMethod::Get,
            url: url.into(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::None,
        }
    }

    fn raw(text: &str) -> Body {
        Body::Raw {
            kind: RawKind::Json,
            text: text.into(),
        }
    }

    fn headers(request: &Request) -> Vec<(String, String)> {
        request
            .resolve()
            .headers
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.into_owned()))
            .collect()
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn url_gets_scheme_and_only_active_params() {
        let mut request = get("example.com/a");
        request.params = vec![kv("q", "1"), disabled("off", "2"), kv("", "3")];
        let resolved = request.resolve();
        assert_eq!(resolved.url, "https://example.com/a");
        assert_eq!(resolved.query, vec![("q", "1")]);
    }

    #[test]
    fn disabled_and_unnamed_headers_are_dropped() {
        let mut request = get("https://example.com");
        request.headers = vec![kv("A", "1"), disabled("B", "2"), kv("", "3"), kv("A", "4")];
        assert_eq!(headers(&request), pairs(&[("A", "1"), ("A", "4")]));
    }

    #[test]
    fn raw_body_adds_content_type_unless_set() {
        let mut request = Request {
            method: HttpMethod::Post,
            body: raw("{}"),
            ..get("https://example.com")
        };
        assert_eq!(
            headers(&request),
            pairs(&[("Content-Type", "application/json")])
        );
        assert_eq!(request.resolve().body, ResolvedBody::Raw("{}"));

        request.headers = vec![kv("content-type", "application/vnd.api+json")];
        assert_eq!(
            headers(&request),
            pairs(&[("content-type", "application/vnd.api+json")])
        );
    }

    #[test]
    fn form_data_drops_content_type_and_inactive_fields() {
        let request = Request {
            method: HttpMethod::Post,
            headers: vec![kv("Content-Type", "text/plain"), kv("X-A", "1")],
            body: Body::FormData(vec![kv("a", "1"), disabled("b", "2")]),
            ..get("https://example.com")
        };
        let resolved = request.resolve();
        assert_eq!(headers(&request), pairs(&[("X-A", "1")]));
        assert_eq!(resolved.body, ResolvedBody::FormData(vec![("a", "1")]));
    }

    #[test]
    fn raw_kinds_send_their_content_type() {
        for (kind, mime) in [
            (RawKind::Text, "text/plain; charset=utf-8"),
            (RawKind::Xml, "application/xml"),
            (RawKind::Html, "text/html"),
        ] {
            let request = Request {
                method: HttpMethod::Post,
                body: Body::Raw {
                    kind,
                    text: "x".into(),
                },
                ..get("https://example.com")
            };
            assert_eq!(headers(&request), pairs(&[("Content-Type", mime)]));
        }
    }

    #[test]
    fn url_encoded_adds_content_type_unless_set() {
        let mut request = Request {
            method: HttpMethod::Post,
            headers: vec![kv("X-A", "1")],
            body: Body::UrlEncoded(vec![kv("a", "1 2"), disabled("b", "2"), kv("", "3")]),
            ..get("https://example.com")
        };
        assert_eq!(
            headers(&request),
            pairs(&[
                ("X-A", "1"),
                ("Content-Type", "application/x-www-form-urlencoded")
            ])
        );
        assert_eq!(
            request.resolve().body,
            ResolvedBody::UrlEncoded(vec![("a", "1 2")])
        );

        request.headers = vec![kv(
            "content-type",
            "application/x-www-form-urlencoded; charset=utf-8",
        )];
        assert_eq!(
            headers(&request),
            pairs(&[(
                "content-type",
                "application/x-www-form-urlencoded; charset=utf-8"
            )])
        );
    }

    #[test]
    fn api_key_is_added_unless_header_exists() {
        let mut request = get("https://example.com");
        request.auth = Auth::ApiKey {
            key: "X-Key".into(),
            value: "secret".into(),
            location: ApiKeyLocation::Header,
        };
        assert_eq!(headers(&request), pairs(&[("X-Key", "secret")]));
        assert!(request.resolve().query.is_empty());

        request.headers = vec![kv("x-key", "mine")];
        assert_eq!(headers(&request), pairs(&[("x-key", "mine")]));

        request.headers.clear();
        request.auth = Auth::ApiKey {
            key: String::new(),
            value: "secret".into(),
            location: ApiKeyLocation::Header,
        };
        assert!(headers(&request).is_empty());
    }

    #[test]
    fn api_key_in_query_is_appended_unless_param_exists() {
        let mut request = get("example.com/a");
        request.params = vec![kv("q", "1")];
        request.headers = vec![kv("api_key", "header")];
        request.auth = Auth::ApiKey {
            key: "api_key".into(),
            value: "secret".into(),
            location: ApiKeyLocation::Query,
        };
        let resolved = request.resolve();
        assert_eq!(resolved.url, "https://example.com/a");
        assert_eq!(resolved.query, vec![("q", "1"), ("api_key", "secret")]);
        assert_eq!(headers(&request), pairs(&[("api_key", "header")]));

        request.params.push(kv("api_key", "mine"));
        assert_eq!(
            request.resolve().query,
            vec![("q", "1"), ("api_key", "mine")]
        );

        request.params[1] = disabled("api_key", "mine");
        assert_eq!(
            request.resolve().query,
            vec![("q", "1"), ("api_key", "secret")]
        );

        request.params[1] = kv("API_KEY", "other");
        assert_eq!(
            request.resolve().query,
            vec![("q", "1"), ("API_KEY", "other"), ("api_key", "secret")]
        );

        request.params.truncate(1);
        request.auth = Auth::ApiKey {
            key: String::new(),
            value: "secret".into(),
            location: ApiKeyLocation::Query,
        };
        assert_eq!(request.resolve().query, vec![("q", "1")]);
    }

    #[test]
    fn bearer_and_basic_auth() {
        let mut request = get("https://example.com");
        request.auth = Auth::Bearer { token: "t".into() };
        assert_eq!(headers(&request), pairs(&[("Authorization", "Bearer t")]));

        request.auth = Auth::Bearer {
            token: String::new(),
        };
        assert!(headers(&request).is_empty());

        request.auth = Auth::Basic {
            username: String::new(),
            password: "p".into(),
        };
        assert_eq!(request.resolve().basic_auth, Some(("", "p")));

        request.auth = Auth::Basic {
            username: String::new(),
            password: String::new(),
        };
        assert_eq!(request.resolve().basic_auth, None);
    }

    #[test]
    fn authorization_header_overrides_auth_tab() {
        let mut request = get("https://example.com");
        request.headers = vec![kv("authorization", "Custom x")];

        request.auth = Auth::Bearer { token: "t".into() };
        assert_eq!(headers(&request), pairs(&[("authorization", "Custom x")]));

        request.auth = Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        };
        assert_eq!(request.resolve().basic_auth, None);

        request.headers = vec![disabled("Authorization", "Custom x")];
        assert_eq!(request.resolve().basic_auth, Some(("u", "p")));
    }
}
