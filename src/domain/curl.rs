use super::{
    request::{HttpMethod, Request},
    resolved::{ResolvedBody, ResolvedRequest},
};

/// Builds a shell command that sends the same request as `http::send`.
pub fn to_curl(request: &Request) -> String {
    let resolved = request.resolve();
    let mut args = vec!["curl".to_string()];

    // curl rejects `--head` together with a body, and a HEAD body has no
    // defined meaning, so it's left out.
    let body = if resolved.method == HttpMethod::Head {
        &ResolvedBody::None
    } else {
        &resolved.body
    };
    let implied = if matches!(body, ResolvedBody::None) {
        HttpMethod::Get
    } else {
        HttpMethod::Post
    };
    if resolved.method == HttpMethod::Head {
        args.push("--head".into());
    } else if resolved.method != implied {
        args.push(format!("-X {}", resolved.method.as_str()));
    }

    args.push(quote(&url(&resolved)));

    if let Some((username, password)) = resolved.basic_auth {
        args.push(format!("-u {}", quote(&format!("{username}:{password}"))));
    }

    for (key, value) in &resolved.headers {
        // `-H 'Name:'` would remove the header; `Name;` sends it empty.
        let header = if value.is_empty() {
            format!("{key};")
        } else {
            format!("{key}: {value}")
        };
        args.push(format!("-H {}", quote(&header)));
    }

    match body {
        ResolvedBody::None => {}
        // `--form-string` doesn't treat values starting with `@` or `<` as files.
        ResolvedBody::FormData(fields) => args.extend(
            fields
                .iter()
                .map(|(key, value)| format!("--form-string {}", quote(&format!("{key}={value}")))),
        ),
        ResolvedBody::Raw(text) => args.push(format!("--data-raw {}", quote(text))),
    }

    args.join(" \\\n  ")
}

fn url(resolved: &ResolvedRequest) -> String {
    if resolved.query.is_empty() {
        return resolved.url.to_string();
    }
    match url::Url::parse(&resolved.url) {
        Ok(mut url) => {
            url.query_pairs_mut().extend_pairs(&resolved.query);
            url.into()
        }
        Err(_) => resolved.url.to_string(),
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::{Auth, Body, KeyValue, RawKind};

    fn kv(key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled: true,
            key: key.into(),
            value: value.into(),
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

    #[test]
    fn plain_get() {
        assert_eq!(
            to_curl(&get("example.com/a")),
            "curl \\\n  'https://example.com/a'"
        );
    }

    #[test]
    fn params_are_appended_and_encoded() {
        let mut request = get("https://example.com/search?lang=en");
        request.params = vec![kv("q", "a b&c")];
        assert_eq!(
            to_curl(&request),
            "curl \\\n  'https://example.com/search?lang=en&q=a+b%26c'"
        );
    }

    #[test]
    fn raw_body_with_quotes() {
        let request = Request {
            method: HttpMethod::Post,
            body: Body::Raw {
                kind: RawKind::Json,
                text: r#"{"name": "it's"}"#.into(),
            },
            ..get("https://example.com")
        };
        assert_eq!(
            to_curl(&request),
            "curl \\\n  'https://example.com' \\\n  -H 'Content-Type: application/json' \\\n  \
             --data-raw '{\"name\": \"it'\\''s\"}'"
        );
    }

    #[test]
    fn method_flag_only_when_not_implied() {
        let mut request = get("https://example.com");
        assert!(!to_curl(&request).contains("-X"));
        request.method = HttpMethod::Delete;
        assert!(to_curl(&request).contains("-X DELETE"));

        request.method = HttpMethod::Get;
        request.body = Body::Raw {
            kind: RawKind::Text,
            text: String::new(),
        };
        assert!(to_curl(&request).contains("-X GET"));
        request.method = HttpMethod::Post;
        assert!(!to_curl(&request).contains("-X"));
    }

    #[test]
    fn head_leaves_body_out() {
        let request = Request {
            method: HttpMethod::Head,
            body: Body::Raw {
                kind: RawKind::Text,
                text: "x".into(),
            },
            ..get("https://example.com")
        };
        let curl = to_curl(&request);
        assert!(curl.contains("--head"));
        assert!(!curl.contains("--data-raw"));
    }

    #[test]
    fn basic_auth_uses_user_flag() {
        let mut request = get("https://example.com");
        request.auth = Auth::Basic {
            username: "u".into(),
            password: "p'q".into(),
        };
        assert!(to_curl(&request).contains("-u 'u:p'\\''q'"));
    }

    #[test]
    fn empty_header_and_form_fields() {
        let request = Request {
            method: HttpMethod::Post,
            headers: vec![kv("X-Empty", "")],
            body: Body::FormData(vec![kv("file", "@/etc/passwd")]),
            ..get("https://example.com")
        };
        let curl = to_curl(&request);
        assert!(curl.contains("-H 'X-Empty;'"));
        assert!(curl.contains("--form-string 'file=@/etc/passwd'"));
        assert!(!curl.contains("-X"));
    }
}
