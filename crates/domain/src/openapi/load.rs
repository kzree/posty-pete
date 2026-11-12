//! Getting spec text from a file or a URL, with a size cap, and summarizing
//! the result for the import preview.

use std::{
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use reqwest::header::{ACCEPT, HeaderValue};
use tokio::task::JoinHandle;

use super::{BASE_URL_VARIABLE, ImportError, ImportResult, import};
use crate::{
    collection::{Item, SourceLocation},
    send_error::SendError,
    url_scheme::with_default_scheme,
};

/// The largest spec read or downloaded. YAML aliases expand in memory, so
/// the text has to be bounded before parsing.
pub const MAX_SPEC_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    InvalidUrl,
    Unreachable(SendError),
    Status { code: u16, reason: Option<String> },
    ReadFile(String),
    TooLarge,
    NotText,
    Import(ImportError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidUrl => f.write_str("Enter an http:// or https:// URL."),
            Self::Unreachable(SendError {
                hint: Some(hint), ..
            }) => write!(f, "Couldn't reach the URL. {hint}"),
            Self::Unreachable(error) => write!(f, "Couldn't reach the URL: {}.", error.message),
            Self::Status {
                code,
                reason: Some(reason),
            } => write!(f, "The server responded with {code} {reason}."),
            Self::Status { code, reason: None } => {
                write!(f, "The server responded with status {code}.")
            }
            Self::ReadFile(message) => write!(f, "Couldn't read the file: {message}."),
            Self::TooLarge => write!(
                f,
                "The spec is larger than {} MB, the most that can be imported.",
                MAX_SPEC_BYTES / (1024 * 1024)
            ),
            Self::NotText => f.write_str("The spec isn't UTF-8 text."),
            Self::Import(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<ImportError> for LoadError {
    fn from(error: ImportError) -> Self {
        Self::Import(error)
    }
}

/// The URL to fetch from what was typed, adding a scheme if there's none.
pub fn parse_url(input: &str) -> Result<url::Url, LoadError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(LoadError::InvalidUrl);
    }
    let url = url::Url::parse(&with_default_scheme(input)).map_err(|_| LoadError::InvalidUrl)?;
    let web = matches!(url.scheme(), "http" | "https");
    if !web || url.host_str().is_none_or(str::is_empty) {
        return Err(LoadError::InvalidUrl);
    }
    Ok(url)
}

/// Reads and converts the spec at `path`. Blocks, so run it off the UI thread.
pub fn load_file(path: PathBuf) -> Result<ImportResult, LoadError> {
    let text = read_file(&path, MAX_SPEC_BYTES)?;
    Ok(import(&text, SourceLocation::File(path))?)
}

fn read_file(path: &Path, limit: u64) -> Result<String, LoadError> {
    let read_error = |error: std::io::Error| LoadError::ReadFile(error.to_string());
    let file = File::open(path).map_err(read_error)?;
    if file.metadata().map_err(read_error)?.len() > limit {
        return Err(LoadError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    if bytes.len() as u64 > limit {
        return Err(LoadError::TooLarge);
    }
    decode(bytes)
}

fn decode(bytes: Vec<u8>) -> Result<String, LoadError> {
    String::from_utf8(bytes).map_err(|_| LoadError::NotText)
}

/// Downloads the spec text on the shared runtime. Aborting the handle stops
/// the download. The text goes nowhere else and isn't kept.
pub fn spawn_fetch(
    client: reqwest::Client,
    url: url::Url,
) -> JoinHandle<Result<String, LoadError>> {
    crate::http::runtime().spawn(fetch(client, url, MAX_SPEC_BYTES))
}

async fn fetch(client: reqwest::Client, url: url::Url, limit: u64) -> Result<String, LoadError> {
    let unreachable =
        |error: reqwest::Error| LoadError::Unreachable(SendError::from_error(&error.into()));
    let accept = HeaderValue::from_static(
        "application/json, application/yaml;q=0.9, text/yaml;q=0.9, */*;q=0.5",
    );
    let mut response = client
        .get(url)
        .header(ACCEPT, accept)
        .send()
        .await
        .map_err(unreachable)?;
    let status = response.status();
    if !status.is_success() {
        return Err(LoadError::Status {
            code: status.as_u16(),
            reason: status.canonical_reason().map(str::to_owned),
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(LoadError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(unreachable)? {
        if (bytes.len() + chunk.len()) as u64 > limit {
            return Err(LoadError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    decode(bytes)
}

/// Converts downloaded spec text. Can take a while on a big spec, so run it
/// off the UI thread.
pub fn import_downloaded(text: &str, url: &url::Url) -> Result<ImportResult, LoadError> {
    Ok(import(text, SourceLocation::Url(url.to_string()))?)
}

/// What the import dialog shows before importing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub name: String,
    /// Like "12 requests, 3 folders".
    pub counts: String,
    pub base_url: Option<String>,
    pub warnings: Vec<String>,
}

pub fn preview(result: &ImportResult) -> Preview {
    let collection = &result.collection;
    let (requests, folders) = count(&collection.items);
    let base_url = collection
        .variables
        .iter()
        .find(|variable| variable.key == BASE_URL_VARIABLE)
        .map(|variable| variable.value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    Preview {
        name: collection.name.clone(),
        counts: counts_text(requests, folders),
        base_url,
        warnings: result.warnings.clone(),
    }
}

fn count(items: &[Item]) -> (usize, usize) {
    items
        .iter()
        .fold((0, 0), |(requests, folders), item| match item {
            Item::Request(_) => (requests + 1, folders),
            Item::Folder(folder) => {
                let (inner_requests, inner_folders) = count(&folder.items);
                (requests + inner_requests, folders + 1 + inner_folders)
            }
        })
}

fn counts_text(requests: usize, folders: usize) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let requests = match requests {
        0 => "No requests".to_string(),
        n => plural(n, "request", "requests"),
    };
    match folders {
        0 => requests,
        n => format!("{requests}, {}", plural(n, "folder", "folders")),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write as _,
        net::{SocketAddr, TcpListener},
        thread,
        time::Duration,
    };

    use super::*;
    use crate::{
        http::{ClientConfig, ProxyConfig, client},
        settings::RequestSettings,
        store::test_support::TempDir,
    };

    const SPEC: &str = r#"
openapi: 3.0.3
info: {title: Pets, version: "1"}
servers: [{url: "https://pets.example.com/v1"}]
tags: [{name: pets}]
paths:
  /pets:
    get: {summary: List pets, tags: [pets]}
    post: {summary: Add pet, tags: [pets]}
  /health:
    get: {summary: Health}
"#;

    fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        fs::create_dir_all(dir.path()).unwrap();
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(counts_text(0, 0), "No requests");
        assert_eq!(counts_text(1, 0), "1 request");
        assert_eq!(counts_text(1, 1), "1 request, 1 folder");
        assert_eq!(counts_text(12, 3), "12 requests, 3 folders");
        assert_eq!(counts_text(0, 2), "No requests, 2 folders");
    }

    #[test]
    fn preview_summarizes_the_collection() {
        let result = import(SPEC, SourceLocation::File("/specs/pets.yaml".into())).unwrap();
        let preview = preview(&result);
        assert_eq!(preview.name, "Pets");
        assert_eq!(preview.counts, "3 requests, 1 folder");
        assert_eq!(
            preview.base_url.as_deref(),
            Some("https://pets.example.com/v1")
        );
        assert!(preview.warnings.is_empty());
    }

    #[test]
    fn preview_without_servers_has_no_base_url_and_warns() {
        let spec = r#"{"openapi": "3.0.0", "info": {"title": "T", "version": "1"}, "paths": {}}"#;
        let result = import(spec, SourceLocation::File("/t.json".into())).unwrap();
        let preview = preview(&result);
        assert_eq!(preview.base_url, None);
        assert_eq!(preview.counts, "No requests");
        assert!(!preview.warnings.is_empty());
    }

    #[test]
    fn urls_get_a_scheme_and_must_be_web_urls() {
        assert_eq!(
            parse_url(" petstore.swagger.io/v2/swagger.json ")
                .unwrap()
                .as_str(),
            "https://petstore.swagger.io/v2/swagger.json"
        );
        assert_eq!(
            parse_url("localhost:8080/openapi.yaml").unwrap().as_str(),
            "http://localhost:8080/openapi.yaml"
        );
        for bad in [
            "",
            "   ",
            "ftp://example.com/spec",
            "file:///etc/spec.json",
            "https://",
        ] {
            assert_eq!(parse_url(bad), Err(LoadError::InvalidUrl), "{bad:?}");
        }
    }

    #[test]
    fn error_wording() {
        let cases = [
            (LoadError::InvalidUrl, "Enter an http:// or https:// URL."),
            (
                LoadError::Unreachable(SendError::new("connection reset")),
                "Couldn't reach the URL: connection reset.",
            ),
            (
                LoadError::Unreachable(SendError {
                    message: "dns error".into(),
                    hint: Some("The host name couldn't be found.".into()),
                }),
                "Couldn't reach the URL. The host name couldn't be found.",
            ),
            (
                LoadError::Status {
                    code: 404,
                    reason: Some("Not Found".into()),
                },
                "The server responded with 404 Not Found.",
            ),
            (
                LoadError::Status {
                    code: 599,
                    reason: None,
                },
                "The server responded with status 599.",
            ),
            (
                LoadError::ReadFile("permission denied".into()),
                "Couldn't read the file: permission denied.",
            ),
            (
                LoadError::TooLarge,
                "The spec is larger than 20 MB, the most that can be imported.",
            ),
            (LoadError::NotText, "The spec isn't UTF-8 text."),
            (
                LoadError::Import(ImportError::NotOpenApi),
                "This isn't an OpenAPI or Swagger document.",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn load_file_imports_and_records_the_path() {
        let dir = TempDir::new();
        let path = write(&dir, "pets.yaml", SPEC.as_bytes());
        let result = load_file(path.clone()).unwrap();
        assert_eq!(result.collection.name, "Pets");
        let source = result.collection.source.unwrap();
        assert_eq!(source.location, SourceLocation::File(path));
    }

    #[test]
    fn load_file_reports_missing_files_and_bad_documents() {
        let dir = TempDir::new();
        let missing = load_file(dir.path().join("nope.json")).unwrap_err();
        assert!(matches!(missing, LoadError::ReadFile(_)), "{missing:?}");

        let other = write(&dir, "other.json", br#"{"name": "not a spec"}"#);
        assert_eq!(
            load_file(other).unwrap_err(),
            LoadError::Import(ImportError::NotOpenApi)
        );

        let binary = write(&dir, "binary.json", &[0xff, 0xfe, 0x00]);
        assert_eq!(load_file(binary).unwrap_err(), LoadError::NotText);
    }

    #[test]
    fn files_over_the_cap_are_refused_before_parsing() {
        let dir = TempDir::new();
        let path = write(&dir, "big.json", &[b' '; 11]);
        assert_eq!(read_file(&path, 10), Err(LoadError::TooLarge));
        assert_eq!(read_file(&path, 11).unwrap().len(), 11);
    }

    fn test_client() -> reqwest::Client {
        let config = ClientConfig {
            timeout: Some(Duration::from_secs(5)),
            proxy: ProxyConfig::Off,
            ..ClientConfig::from_settings(&RequestSettings::default()).unwrap()
        };
        client(&config).unwrap()
    }

    /// Answers one connection with `reply` and hands back the request it got.
    fn serve_once(reply: Vec<u8>) -> (SocketAddr, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            let read = stream.read(&mut buf).unwrap_or(0);
            let _ = stream.write_all(&reply);
            String::from_utf8_lossy(&buf[..read]).into_owned()
        });
        (addr, handle)
    }

    fn ok_reply(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/yaml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn fetch_blocking(url: &str, limit: u64) -> Result<String, LoadError> {
        let url = parse_url(url).unwrap();
        futures::executor::block_on(crate::http::runtime().spawn(fetch(test_client(), url, limit)))
            .unwrap()
    }

    #[test]
    fn fetch_downloads_the_spec_and_imports_it() {
        let (addr, server) = serve_once(ok_reply(SPEC));
        let url = parse_url(&format!("http://{addr}/openapi.yaml")).unwrap();
        let text = futures::executor::block_on(spawn_fetch(test_client(), url.clone()))
            .unwrap()
            .unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /openapi.yaml "), "{request}");
        assert!(request.to_lowercase().contains("accept: application/json"));

        let result = import_downloaded(&text, &url).unwrap();
        assert_eq!(result.collection.name, "Pets");
        assert_eq!(
            result.collection.source.unwrap().location,
            SourceLocation::Url(format!("http://{addr}/openapi.yaml"))
        );
    }

    #[test]
    fn fetch_reports_error_statuses() {
        let reply = b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        let (addr, _server) = serve_once(reply.to_vec());
        assert_eq!(
            fetch_blocking(&format!("http://{addr}/missing"), MAX_SPEC_BYTES),
            Err(LoadError::Status {
                code: 404,
                reason: Some("Not Found".into())
            })
        );
    }

    #[test]
    fn fetch_refuses_downloads_over_the_cap() {
        let (addr, _server) = serve_once(ok_reply("0123456789ab"));
        assert_eq!(
            fetch_blocking(&format!("http://{addr}/big"), 10),
            Err(LoadError::TooLarge)
        );

        let chunked = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n6\r\n012345\r\n6\r\n6789ab\r\n0\r\n\r\n";
        let (addr, _server) = serve_once(chunked.to_vec());
        assert_eq!(
            fetch_blocking(&format!("http://{addr}/big"), 10),
            Err(LoadError::TooLarge)
        );
    }

    #[test]
    fn fetch_reports_unreachable_servers() {
        let addr = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let error = fetch_blocking(&format!("http://{addr}/"), MAX_SPEC_BYTES).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Couldn't reach the URL. Nothing accepted the connection."),
            "{error}"
        );
    }
}
