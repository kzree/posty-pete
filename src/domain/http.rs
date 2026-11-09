use std::{sync::OnceLock, time::Instant};

use tokio::{runtime::Runtime, task::JoinHandle};

use anyhow::Context as _;
use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
    multipart::Form,
    redirect::{Attempt, Policy},
};

use super::{
    request::{Auth, Body, KeyValue, Request},
    response::Response,
    url_scheme::with_default_scheme,
};

fn form_data(fields: &[KeyValue]) -> Form {
    active(fields).fold(Form::new(), |form, (key, value)| {
        form.text(key.to_owned(), value.to_owned())
    })
}

fn active(items: &[KeyValue]) -> impl Iterator<Item = (&str, &str)> {
    items
        .iter()
        .filter(|kv| kv.enabled && !kv.key.is_empty())
        .map(|kv| (kv.key.as_str(), kv.value.as_str()))
}

fn header_map(items: &[KeyValue]) -> anyhow::Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (key, value) in active(items) {
        let name = HeaderName::from_bytes(key.as_bytes())
            .with_context(|| format!("invalid header name: {key:?}"))?;
        let value = HeaderValue::from_str(value)
            .with_context(|| format!("invalid value for header {key:?}"))?;
        map.append(name, value); // repeated headers are allowed
    }
    Ok(map)
}

fn same_origin(a: &reqwest::Url, b: &reqwest::Url) -> bool {
    a.origin() == b.origin()
}

/// Follows redirects only within the same origin; a cross-origin 3xx is returned as the response.
pub fn redirect_policy() -> Policy {
    let limited = Policy::default();
    Policy::custom(move |attempt: Attempt| {
        let same = attempt
            .previous()
            .last()
            .is_some_and(|prev| same_origin(prev, attempt.url()));
        if same {
            limited.redirect(attempt)
        } else {
            attempt.stop()
        }
    })
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start tokio runtime")
    })
}

fn build_request(client: &reqwest::Client, request: Request) -> anyhow::Result<reqwest::Request> {
    let query: Vec<_> = active(&request.params).collect();
    let mut headers = header_map(&request.headers)?;
    let user_set_authorization = headers.contains_key(AUTHORIZATION);

    if let Auth::ApiKey { key, value } = &request.auth
        && !key.is_empty()
    {
        let name = HeaderName::from_bytes(key.as_bytes())
            .with_context(|| format!("invalid API key header name: {key:?}"))?;
        if !headers.contains_key(&name) {
            let value = HeaderValue::from_str(value)
                .with_context(|| format!("invalid API key value for {key:?}"))?;
            headers.insert(name, value);
        }
    }

    match &request.body {
        // reqwest sets the multipart Content-Type with its boundary.
        Body::FormData(_) => {
            headers.remove(CONTENT_TYPE);
        }
        Body::Raw { kind, .. } if !headers.contains_key(CONTENT_TYPE) => {
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(kind.mime().as_ref())?);
        }
        _ => {}
    }

    let url = with_default_scheme(&request.url);

    let builder = client
        .request(request.method.into(), url.as_ref())
        .query(&query)
        .headers(headers);

    let builder = match &request.auth {
        Auth::Basic { username, password }
            if !user_set_authorization && !(username.is_empty() && password.is_empty()) =>
        {
            builder.basic_auth(username, Some(password))
        }
        Auth::Bearer { token } if !user_set_authorization && !token.is_empty() => {
            builder.bearer_auth(token)
        }
        _ => builder,
    };

    let builder = match request.body {
        Body::None => builder,
        Body::FormData(fields) => builder.multipart(form_data(&fields)),
        Body::Raw { text, .. } => builder.body(text),
    };

    Ok(builder.build()?)
}

pub async fn send(client: reqwest::Client, request: Request) -> anyhow::Result<Response> {
    let start = Instant::now();
    let req = build_request(&client, request)?;
    let mut res = client.execute(req).await?;

    let status = res.status().as_u16();
    let headers = res
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<binary>").to_string()))
        .collect();
    let bytes = read_body(&mut res, MAX_BODY_BYTES).await?;
    let body = String::from_utf8_lossy(&bytes).into_owned();

    Ok(Response {
        status,
        headers,
        body,
        elapsed: start.elapsed(),
    })
}

const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

async fn read_body(res: &mut reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        append_limited(&mut buf, &chunk, limit)?;
    }
    Ok(buf)
}

fn append_limited(buf: &mut Vec<u8>, chunk: &[u8], limit: usize) -> anyhow::Result<()> {
    if buf.len() + chunk.len() > limit {
        anyhow::bail!("response exceeds {} MB", limit / (1024 * 1024));
    }
    buf.extend_from_slice(chunk);
    Ok(())
}

pub fn spawn_send(
    client: reqwest::Client,
    request: Request,
) -> JoinHandle<anyhow::Result<Response>> {
    runtime().spawn(send(client, request))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::HttpMethod;

    fn header(key: &str, value: &str, enabled: bool) -> KeyValue {
        KeyValue {
            enabled,
            key: key.into(),
            value: value.into(),
        }
    }

    fn authorization(headers: Vec<KeyValue>, auth: Auth) -> Option<String> {
        let request = Request {
            method: HttpMethod::Get,
            url: "example.com".into(),
            params: vec![],
            headers,
            body: Body::None,
            auth,
        };
        let built = build_request(&reqwest::Client::new(), request).unwrap();
        built
            .headers()
            .get(AUTHORIZATION)
            .map(|v| v.to_str().unwrap().to_owned())
    }

    fn basic(username: &str, password: &str) -> Auth {
        Auth::Basic {
            username: username.into(),
            password: password.into(),
        }
    }

    #[test]
    fn body_limit() {
        let mut buf = Vec::new();
        append_limited(&mut buf, b"abc", 5).unwrap();
        append_limited(&mut buf, b"de", 5).unwrap();
        assert_eq!(buf, b"abcde");

        let err = append_limited(&mut buf, b"f", 5).unwrap_err();
        assert!(err.to_string().starts_with("response exceeds"), "{err}");
        assert_eq!(buf, b"abcde");

        let mut buf = Vec::new();
        assert!(append_limited(&mut buf, b"abcdef", 5).is_err());
        assert!(buf.is_empty());
    }

    fn origins_match(a: &str, b: &str) -> bool {
        same_origin(&a.parse().unwrap(), &b.parse().unwrap())
    }

    #[test]
    fn same_origin_comparison() {
        assert!(origins_match(
            "https://a.example/x",
            "https://a.example/y?z=1"
        ));
        assert!(origins_match(
            "https://a.example/",
            "https://a.example:443/"
        ));
        assert!(origins_match(
            "http://localhost:4000/a",
            "http://LOCALHOST:4000/b"
        ));
        assert!(!origins_match(
            "http://localhost:4000/",
            "http://localhost:4001/"
        ));
        assert!(!origins_match("https://a.example/", "https://b.example/"));
        assert!(!origins_match(
            "https://a.example/",
            "https://sub.a.example/"
        ));
        assert!(!origins_match("https://a.example/", "http://a.example/"));
        assert!(!origins_match("http://a.example/", "https://a.example/"));
    }

    #[test]
    fn basic_auth_credentials() {
        for (username, password, expected) in [
            ("user", "pass", Some("Basic dXNlcjpwYXNz")),
            ("", "pass", Some("Basic OnBhc3M=")),
            ("user", "", Some("Basic dXNlcjo=")),
            ("", "", None),
        ] {
            assert_eq!(
                authorization(vec![], basic(username, password)).as_deref(),
                expected,
                "{username:?}:{password:?}"
            );
        }
    }

    #[test]
    fn manual_authorization_header_overrides_auth_tab() {
        let headers = vec![header("Authorization", "Token abc", true)];
        assert_eq!(
            authorization(headers.clone(), basic("user", "pass")).as_deref(),
            Some("Token abc")
        );
        let bearer = Auth::Bearer {
            token: "xyz".into(),
        };
        assert_eq!(authorization(headers, bearer).as_deref(), Some("Token abc"));
    }

    #[test]
    fn disabled_authorization_header_does_not_suppress_auth() {
        let headers = vec![header("Authorization", "Token abc", false)];
        assert_eq!(
            authorization(headers.clone(), basic("", "pass")).as_deref(),
            Some("Basic OnBhc3M=")
        );
        let bearer = Auth::Bearer {
            token: "xyz".into(),
        };
        assert_eq!(
            authorization(headers, bearer).as_deref(),
            Some("Bearer xyz")
        );
    }
}
