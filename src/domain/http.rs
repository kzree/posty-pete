use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use anyhow::Ok;
use tokio::{runtime::Runtime, task::JoinHandle};

use crate::domain::request::Request;

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed: Duration,
}

impl Response {
    pub fn pretty_body(&self) -> String {
        serde_json::from_str::<serde_json::Value>(&self.body)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| self.body.clone())
    }
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
    let res = client
        .request(request.method.into(), &request.url)
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

pub fn spawn_send(
    client: reqwest::Client,
    request: Request,
) -> JoinHandle<anyhow::Result<Response>> {
    runtime().spawn(send(client, request))
}
