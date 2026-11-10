use std::borrow::Cow;

use super::{
    request::{Auth, Body, HttpMethod, KeyValue, Request},
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
    Raw(&'a str),
}

impl Request {
    pub fn resolve(&self) -> ResolvedRequest<'_> {
        let is_form = matches!(self.body, Body::FormData(_));
        let mut headers: Vec<(&str, Cow<str>)> = active(&self.headers)
            // The multipart Content-Type needs the boundary the sender picks.
            .filter(|(key, _)| !(is_form && key.eq_ignore_ascii_case("content-type")))
            .map(|(key, value)| (key, Cow::Borrowed(value)))
            .collect();
        let has_header = |headers: &[(&str, Cow<str>)], name: &str| {
            headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name))
        };
        let user_set_authorization = has_header(&headers, "authorization");

        if let Auth::ApiKey { key, value } = &self.auth
            && !key.is_empty()
            && !has_header(&headers, key)
        {
            headers.push((key, Cow::Borrowed(value)));
        }
        if let Body::Raw { kind, .. } = &self.body
            && !has_header(&headers, "content-type")
        {
            headers.push(("Content-Type", Cow::Owned(kind.mime().to_string())));
        }

        let mut basic_auth = None;
        match &self.auth {
            Auth::Basic { username, password }
                if !user_set_authorization && !(username.is_empty() && password.is_empty()) =>
            {
                basic_auth = Some((username.as_str(), password.as_str()));
            }
            Auth::Bearer { token } if !user_set_authorization && !token.is_empty() => {
                headers.push(("Authorization", Cow::Owned(format!("Bearer {token}"))));
            }
            _ => {}
        }

        ResolvedRequest {
            method: self.method,
            url: with_default_scheme(&self.url),
            query: active(&self.params).collect(),
            headers,
            basic_auth,
            body: match &self.body {
                Body::None => ResolvedBody::None,
                Body::FormData(fields) => ResolvedBody::FormData(active(fields).collect()),
                Body::Raw { text, .. } => ResolvedBody::Raw(text),
            },
        }
    }
}

fn active(items: &[KeyValue]) -> impl Iterator<Item = (&str, &str)> {
    items
        .iter()
        .filter(|kv| kv.enabled && !kv.key.is_empty())
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
    fn api_key_is_added_unless_header_exists() {
        let mut request = get("https://example.com");
        request.auth = Auth::ApiKey {
            key: "X-Key".into(),
            value: "secret".into(),
        };
        assert_eq!(headers(&request), pairs(&[("X-Key", "secret")]));

        request.headers = vec![kv("x-key", "mine")];
        assert_eq!(headers(&request), pairs(&[("x-key", "mine")]));

        request.headers.clear();
        request.auth = Auth::ApiKey {
            key: String::new(),
            value: "secret".into(),
        };
        assert!(headers(&request).is_empty());
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
