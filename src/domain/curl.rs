use super::{
    http::active,
    request::{Auth, Body, HttpMethod, Request},
    url_scheme::with_default_scheme,
};

/// Builds a shell command that sends the same request as `http::send`.
pub fn to_curl(request: &Request) -> String {
    let mut args = vec!["curl".to_string()];

    // curl rejects `--head` together with a body, and a HEAD body has no
    // defined meaning, so it's left out.
    let body = if request.method == HttpMethod::Head {
        &Body::None
    } else {
        &request.body
    };
    let has_body = !matches!(body, Body::None);
    let implied = if has_body {
        HttpMethod::Post
    } else {
        HttpMethod::Get
    };
    if request.method == HttpMethod::Head {
        args.push("--head".into());
    } else if request.method != implied {
        args.push(format!("-X {}", request.method.as_str()));
    }

    args.push(quote(&url(request)));

    let mut headers: Vec<(&str, &str)> = active(&request.headers)
        .filter(|(key, _)| {
            !(matches!(body, Body::FormData(_)) && key.eq_ignore_ascii_case("content-type"))
        })
        .collect();
    let has_header = |headers: &[(&str, &str)], name: &str| {
        headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case(name))
    };
    let user_set_authorization = has_header(&headers, "authorization");

    if let Auth::ApiKey { key, value } = &request.auth
        && !key.is_empty()
        && !has_header(&headers, key)
    {
        headers.push((key, value));
    }
    let content_type;
    if let Body::Raw { kind, .. } = body
        && !has_header(&headers, "content-type")
    {
        content_type = kind.mime();
        headers.push(("Content-Type", content_type.as_ref()));
    }

    let bearer;
    match &request.auth {
        Auth::Basic { username, password }
            if !user_set_authorization && !(username.is_empty() && password.is_empty()) =>
        {
            args.push(format!("-u {}", quote(&format!("{username}:{password}"))));
        }
        Auth::Bearer { token } if !user_set_authorization && !token.is_empty() => {
            bearer = format!("Bearer {token}");
            headers.push(("Authorization", &bearer));
        }
        _ => {}
    }

    for (key, value) in headers {
        // `-H 'Name:'` would remove the header; `Name;` sends it empty.
        let header = if value.is_empty() {
            format!("{key};")
        } else {
            format!("{key}: {value}")
        };
        args.push(format!("-H {}", quote(&header)));
    }

    match body {
        Body::None => {}
        // `--form-string` doesn't treat values starting with `@` or `<` as files.
        Body::FormData(fields) => args.extend(
            active(fields)
                .map(|(key, value)| format!("--form-string {}", quote(&format!("{key}={value}")))),
        ),
        Body::Raw { text, .. } => args.push(format!("--data-raw {}", quote(text))),
    }

    args.join(" \\\n  ")
}

fn url(request: &Request) -> String {
    let raw = with_default_scheme(&request.url);
    let mut params = active(&request.params).peekable();
    if params.peek().is_none() {
        return raw.into_owned();
    }
    match url::Url::parse(&raw) {
        Ok(mut url) => {
            url.query_pairs_mut().extend_pairs(params);
            url.into()
        }
        Err(_) => raw.into_owned(),
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::{KeyValue, RawKind};

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
        request.params = vec![
            kv("q", "a b&c"),
            KeyValue {
                enabled: false,
                ..kv("off", "1")
            },
        ];
        assert_eq!(
            to_curl(&request),
            "curl \\\n  'https://example.com/search?lang=en&q=a+b%26c'"
        );
    }

    #[test]
    fn raw_body_with_content_type_and_quotes() {
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
        request.method = HttpMethod::Delete;
        assert!(to_curl(&request).contains("-X DELETE"));
        request.method = HttpMethod::Head;
        assert!(to_curl(&request).contains("--head"));

        request.method = HttpMethod::Get;
        request.body = Body::Raw {
            kind: RawKind::Text,
            text: String::new(),
        };
        assert!(to_curl(&request).contains("-X GET"));
    }

    #[test]
    fn auth() {
        let mut request = get("https://example.com");
        request.auth = Auth::Bearer { token: "t".into() };
        assert!(to_curl(&request).contains("-H 'Authorization: Bearer t'"));

        request.headers = vec![kv("authorization", "Custom x")];
        let curl = to_curl(&request);
        assert!(curl.contains("-H 'authorization: Custom x'"));
        assert!(!curl.contains("Bearer"));

        request.headers.clear();
        request.auth = Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        };
        assert!(to_curl(&request).contains("-u 'u:p'"));

        request.auth = Auth::ApiKey {
            key: "X-Key".into(),
            value: "secret".into(),
        };
        assert!(to_curl(&request).contains("-H 'X-Key: secret'"));
    }

    #[test]
    fn head_leaves_body_out() {
        let request = Request {
            method: HttpMethod::Head,
            body: Body::Raw {
                kind: RawKind::Json,
                text: "{}".into(),
            },
            ..get("https://example.com")
        };
        assert_eq!(
            to_curl(&request),
            "curl \\\n  --head \\\n  'https://example.com'"
        );
    }

    #[test]
    fn form_data_drops_content_type() {
        let request = Request {
            method: HttpMethod::Post,
            headers: vec![kv("Content-Type", "text/plain"), kv("X-Empty", "")],
            body: Body::FormData(vec![kv("file", "@/etc/passwd")]),
            ..get("https://example.com")
        };
        let curl = to_curl(&request);
        assert!(!curl.contains("Content-Type"));
        assert!(curl.contains("-H 'X-Empty;'"));
        assert!(curl.contains("--form-string 'file=@/etc/passwd'"));
    }
}
