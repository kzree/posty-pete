use std::{sync::OnceLock, time::Instant};

use tokio::{runtime::Runtime, task::JoinHandle};

use anyhow::Context as _;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::{
    request::{KeyValue, Request},
    response::Response,
};

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
    let headers = header_map(&request.headers)?;

    let res = client
        .request(request.method.into(), &request.url)
        .query(&query)
        .headers(headers)
        .send()
        .await?;

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
