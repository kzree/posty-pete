use std::{sync::OnceLock, time::Instant};

use tokio::{runtime::Runtime, task::JoinHandle};

use anyhow::Context as _;
use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
    multipart::Form,
};

use super::{
    request::{Auth, Body, KeyValue, Request},
    response::Response,
};

fn form_data(fields: &[KeyValue]) -> Form {
    active(fields).fold(Form::new(), |form, (key, value)| {
        form.text(key.to_owned(), value.to_owned())
    })
}

/// Enabled rows that have a key.
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
        map.append(name, value); // append, not insert: repeated headers are allowed
    }
    Ok(map)
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

pub async fn send(client: reqwest::Client, request: Request) -> anyhow::Result<Response> {
    let start = Instant::now();
    let query: Vec<_> = active(&request.params).collect();
    let mut headers = header_map(&request.headers)?;
    // An Authorization header set in the Headers tab wins over the Auth tab.
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
        // reqwest sets multipart/form-data with the boundary; a user value would break it.
        Body::FormData(_) => {
            headers.remove(CONTENT_TYPE);
        }
        Body::Raw { kind, .. } if !headers.contains_key(CONTENT_TYPE) => {
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(kind.mime().as_ref())?);
        }
        _ => {}
    }

    let builder = client
        .request(request.method.into(), &request.url)
        .query(&query)
        .headers(headers);

    let builder = match &request.auth {
        Auth::Basic { username, password } if !user_set_authorization && !username.is_empty() => {
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

    let res = builder.send().await?;

    let status = res.status().as_u16();
    let headers = res
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<binary>").to_string()))
        .collect();
    let body = res.text().await?;

    Ok(Response {
        status,
        headers,
        body,
        elapsed: start.elapsed(),
    })
}

/// Runs `send` on the tokio runtime. Await the handle from any executor.
pub fn spawn_send(
    client: reqwest::Client,
    request: Request,
) -> JoinHandle<anyhow::Result<Response>> {
    runtime().spawn(send(client, request))
}
