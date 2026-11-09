use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use mime::Mime;
use tokio::{runtime::Runtime, task::JoinHandle};

use crate::domain::request::Request;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyFormat {
    Json,
    Html,
    Xml,
    Css,
    JavaScript,
    Yaml,
    Text,
}

impl BodyFormat {
    pub fn from_content_type(content_type: Option<&str>) -> Self {
        let Some(mime) = content_type.and_then(|ct| ct.parse::<Mime>().ok()) else {
            return Self::Text;
        };

        let (ty, subtype, suffix) = (mime.type_(), mime.subtype(), mime.suffix());

        if subtype == mime::JSON || suffix == Some(mime::JSON) {
            Self::Json
        } else if subtype == mime::XML || suffix == Some(mime::XML) {
            Self::Xml
        } else if ty == mime::TEXT && subtype == mime::HTML {
            Self::Html
        } else if ty == mime::TEXT && subtype == mime::CSS {
            Self::Css
        } else if subtype == mime::JAVASCRIPT {
            Self::JavaScript
        } else if subtype == "yaml" || subtype == "x-yaml" || suffix.is_some_and(|s| s == "yaml") {
            // mime has no YAML constant
            Self::Yaml
        } else {
            Self::Text
        }
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed: Duration,
}

impl Response {
    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())
    }

    pub fn body_format(&self) -> BodyFormat {
        BodyFormat::from_content_type(self.content_type())
    }

    pub fn pretty_body(&self) -> String {
        if self.body_format() != BodyFormat::Json {
            return self.body.clone();
        }
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
