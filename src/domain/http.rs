use std::{sync::OnceLock, time::Instant};

use tokio::{runtime::Runtime, task::JoinHandle};

use super::{request::Request, response::Response};

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
    let query: Vec<(&str, &str)> = request
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .map(|p| (p.key.as_str(), p.value.as_str()))
        .collect();

    let res = client
        .request(request.method.into(), &request.url)
        .query(&query)
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
