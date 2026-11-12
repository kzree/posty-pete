use std::{
    fmt,
    sync::OnceLock,
    time::{Duration, Instant},
};

use tokio::{runtime::Runtime, task::JoinHandle};

use anyhow::Context as _;
use bytes::Bytes;
pub use reqwest::Client;
use reqwest::{
    Proxy,
    header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue},
    multipart::Form,
    redirect::{Attempt, Policy},
};

use super::{
    request::{Body, RawText, Request},
    resolved::{ResolvedBody, is_active},
    response::{Response, header_value_text},
    send_error::SendError,
    settings::{ProxyMode, RequestSettings},
};

fn same_origin(a: &reqwest::Url, b: &reqwest::Url) -> bool {
    a.origin() == b.origin()
}

/// Follows up to `max` redirects within the same origin; a cross-origin 3xx is returned as the response.
fn redirect_policy(max: usize) -> Policy {
    let limited = Policy::limited(max);
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

/// How to build the client, checked and ready to apply.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientConfig {
    /// `None` means no timeout.
    pub timeout: Option<Duration>,
    /// `None` means redirects aren't followed.
    pub max_redirects: Option<usize>,
    pub verify_tls: bool,
    pub proxy: ProxyConfig,
    pub user_agent: Option<HeaderValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyConfig {
    Off,
    /// reqwest's own lookup of the proxy environment variables. Without its
    /// `system-proxy` feature, the OS network settings aren't read.
    FromEnv,
    Url(reqwest::Url),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    ProxyUrl(ProxyUrlError),
    UserAgent,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProxyUrl(error) => error.fmt(f),
            Self::UserAgent => f.write_str("The User-Agent can't contain control characters."),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyUrlError {
    Empty,
    /// The URL parser's reason.
    Invalid(String),
    UnsupportedScheme(String),
}

impl fmt::Display for ProxyUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Enter the proxy's URL."),
            Self::Invalid(reason) => write!(f, "This isn't a valid URL ({reason})."),
            Self::UnsupportedScheme(scheme) => write!(
                f,
                "{scheme}:// proxies aren't supported. Use http:// or https://."
            ),
        }
    }
}

/// Parses a custom proxy's URL. Without a scheme, `http://` is assumed.
pub fn parse_proxy_url(text: &str) -> Result<reqwest::Url, ProxyUrlError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ProxyUrlError::Empty);
    }
    let url = if text.contains("://") {
        reqwest::Url::parse(text)
    } else {
        reqwest::Url::parse(&format!("http://{text}"))
    }
    .map_err(|e| ProxyUrlError::Invalid(e.to_string()))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(ProxyUrlError::UnsupportedScheme(other.to_owned())),
    }
}

/// The default User-Agent header; blank means none.
pub fn parse_user_agent(text: &str) -> Result<Option<HeaderValue>, ConfigError> {
    match text.trim() {
        "" => Ok(None),
        text => HeaderValue::from_str(text)
            .map(Some)
            .map_err(|_| ConfigError::UserAgent),
    }
}

impl ClientConfig {
    pub fn from_settings(settings: &RequestSettings) -> Result<Self, ConfigError> {
        let proxy = match settings.proxy {
            ProxyMode::None => ProxyConfig::Off,
            ProxyMode::System => ProxyConfig::FromEnv,
            ProxyMode::Custom => ProxyConfig::Url(
                parse_proxy_url(&settings.proxy_url).map_err(ConfigError::ProxyUrl)?,
            ),
        };
        let user_agent = parse_user_agent(&settings.user_agent)?;
        let max_redirects = settings.max_redirects.clamp(
            *RequestSettings::MAX_REDIRECTS.start(),
            *RequestSettings::MAX_REDIRECTS.end(),
        );
        Ok(Self {
            timeout: (settings.timeout_secs > 0)
                .then(|| Duration::from_secs(settings.timeout_secs.into())),
            max_redirects: settings.follow_redirects.then_some(max_redirects as usize),
            verify_tls: settings.verify_tls,
            proxy,
            user_agent,
        })
    }
}

/// The client for the current request settings. It's rebuilt only when the
/// settings change what the client does. While they're invalid there is no
/// client to send with, but the last one built is kept for when they're valid again.
pub struct SettingsClient {
    built: Option<(ClientConfig, reqwest::Client)>,
    error: Option<anyhow::Error>,
    #[cfg(test)]
    builds: usize,
}

impl SettingsClient {
    pub fn new(settings: &RequestSettings) -> Self {
        let mut this = Self {
            built: None,
            error: None,
            #[cfg(test)]
            builds: 0,
        };
        this.apply(settings);
        this
    }

    pub fn apply(&mut self, settings: &RequestSettings) {
        let config = match ClientConfig::from_settings(settings) {
            Ok(config) => config,
            Err(error) => {
                self.error = Some(anyhow::anyhow!("{error} Fix it in Settings › Requests."));
                return;
            }
        };
        self.error = None;
        if self
            .built
            .as_ref()
            .is_some_and(|(built, _)| *built == config)
        {
            return;
        }
        #[cfg(test)]
        {
            self.builds += 1;
        }
        match client(&config) {
            Ok(client) => self.built = Some((config, client)),
            Err(error) => {
                self.error =
                    Some(anyhow::Error::from(error).context("couldn't set up the HTTP client"))
            }
        }
    }

    /// The client to send with, or why there is none.
    pub fn get(&self) -> Result<reqwest::Client, &anyhow::Error> {
        match (&self.error, &self.built) {
            (Some(error), _) => Err(error),
            (None, Some((_, client))) => Ok(client.clone()),
            (None, None) => unreachable!("a client is built whenever there's no error"),
        }
    }

    #[cfg(test)]
    fn config(&self) -> Option<&ClientConfig> {
        self.built.as_ref().map(|(config, _)| config)
    }
}

pub fn client(config: &ClientConfig) -> reqwest::Result<reqwest::Client> {
    let redirect = match config.max_redirects {
        Some(max) => redirect_policy(max),
        None => Policy::none(),
    };
    let mut builder = reqwest::Client::builder()
        .redirect(redirect)
        .tls_danger_accept_invalid_certs(!config.verify_tls);
    if let Some(timeout) = config.timeout {
        builder = builder.timeout(timeout);
    }
    builder = match &config.proxy {
        ProxyConfig::Off => builder.no_proxy(),
        ProxyConfig::FromEnv => builder,
        ProxyConfig::Url(url) => builder.proxy(Proxy::all(url.clone())?),
    };
    if let Some(user_agent) = &config.user_agent {
        builder = builder.user_agent(user_agent.clone());
    }
    builder.build()
}

pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start tokio runtime")
    })
}

/// Lets a raw body's shared text be sent without copying it.
struct RawBody(RawText);

impl AsRef<[u8]> for RawBody {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Takes `request` by value so the raw text and form fields move into reqwest.
fn build_request(client: &reqwest::Client, request: Request) -> anyhow::Result<reqwest::Request> {
    let builder = {
        let resolved = request.resolve();

        let mut headers = HeaderMap::new();
        for (key, value) in &resolved.headers {
            let name = HeaderName::from_bytes(key.as_bytes())
                .with_context(|| format!("invalid header name: {key:?}"))?;
            let mut value = HeaderValue::from_str(value)
                .with_context(|| format!("invalid value for header {key:?}"))?;
            value.set_sensitive(name == AUTHORIZATION);
            headers.append(name, value); // repeated headers are allowed
        }

        let builder = client
            .request(resolved.method.into(), resolved.url.as_ref())
            .query(&resolved.query)
            .headers(headers);

        let builder = match resolved.basic_auth {
            Some((username, password)) => builder.basic_auth(username, Some(password)),
            None => builder,
        };

        match resolved.body {
            // The Content-Type is already in `headers`, so `form` keeps it.
            ResolvedBody::UrlEncoded(fields) => builder.form(&fields),
            ResolvedBody::None | ResolvedBody::FormData(_) | ResolvedBody::Raw(_) => builder,
        }
    };

    let builder = match request.body {
        Body::FormData(fields) => {
            let form = fields
                .into_iter()
                .filter(is_active)
                .fold(Form::new(), |form, field| form.text(field.key, field.value));
            builder.multipart(form)
        }
        Body::Raw { text, .. } => builder.body(Bytes::from_owner(RawBody(text))),
        Body::None | Body::UrlEncoded(_) => builder,
    };

    Ok(builder.build()?)
}

pub async fn send(client: reqwest::Client, request: Request) -> Result<Response, SendError> {
    try_send(client, request)
        .await
        .map_err(|error| SendError::from_error(&error))
}

async fn try_send(client: reqwest::Client, request: Request) -> anyhow::Result<Response> {
    let start = Instant::now();
    let req = build_request(&client, request)?;
    let mut res = client.execute(req).await?;

    let status = res.status().as_u16();
    let headers = res
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), header_value_text(v.as_bytes())))
        .collect();
    let body = read_body(&mut res, MAX_BODY_BYTES).await?;

    Ok(Response::new(status, headers, body, start.elapsed()))
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
) -> JoinHandle<Result<Response, SendError>> {
    runtime().spawn(send(client, request))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::{SocketAddr, TcpListener},
        sync::mpsc,
        thread,
    };

    use reqwest::header::CONTENT_TYPE;

    use super::*;
    use crate::request::{Auth, HttpMethod, KeyValue, RawKind};
    use crate::send_error;

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

    #[test]
    fn built_request_has_query_and_content_type() {
        let request = Request {
            method: HttpMethod::Post,
            url: "example.com/a?x=1".into(),
            params: vec![header("q", "a b", true), header("off", "1", false)],
            headers: vec![],
            body: Body::Raw {
                kind: RawKind::Json,
                text: "{}".into(),
            },
            auth: Auth::None,
        };
        let built = build_request(&reqwest::Client::new(), request).unwrap();
        assert_eq!(built.url().as_str(), "https://example.com/a?x=1&q=a+b");
        assert_eq!(
            built.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn built_url_encoded_body() {
        let mut request = Request {
            method: HttpMethod::Post,
            url: "https://example.com".into(),
            params: vec![],
            headers: vec![],
            body: Body::UrlEncoded(vec![
                header("a b", "1&2=3", true),
                header("off", "1", false),
                header("c", "é", true),
            ]),
            auth: Auth::None,
        };
        let built = build_request(&reqwest::Client::new(), request.clone()).unwrap();
        assert_eq!(
            built.headers().get(CONTENT_TYPE).unwrap(),
            "application/x-www-form-urlencoded"
        );
        assert_eq!(
            built.body().and_then(|body| body.as_bytes()),
            Some(&b"a+b=1%262%3D3&c=%C3%A9"[..])
        );

        request.headers = vec![header("Content-Type", "text/plain", true)];
        let built = build_request(&reqwest::Client::new(), request).unwrap();
        let types: Vec<_> = built.headers().get_all(CONTENT_TYPE).iter().collect();
        assert_eq!(types, ["text/plain"]);
    }

    #[test]
    fn built_raw_body_shares_the_text() {
        let text = RawText::from(r#"{"a": 1}"#);
        let request = Request {
            method: HttpMethod::Post,
            url: "https://example.com".into(),
            body: Body::Raw {
                kind: RawKind::Json,
                text: text.clone(),
            },
            ..Request::default()
        };
        let built = build_request(&reqwest::Client::new(), request).unwrap();
        let body = built.body().and_then(|body| body.as_bytes()).unwrap();
        assert_eq!(body, text.as_bytes());
        assert_eq!(body.as_ptr(), text.as_ptr());
    }

    #[test]
    fn form_data_sends_active_fields_with_its_own_content_type() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let mut buf = [0; 4096];
            while !received.ends_with(b"--\r\n") {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0, "connection closed early");
                received.extend_from_slice(&buf[..n]);
            }
            let _ = stream.write_all(OK.as_bytes());
            String::from_utf8(received).unwrap()
        });

        let request = Request {
            method: HttpMethod::Post,
            headers: vec![header("Content-Type", "text/plain", true)],
            body: Body::FormData(vec![
                header("a", "1", true),
                header("off", "2", false),
                header("", "3", true),
                header("b", "é", true),
            ]),
            ..get(format!("http://{addr}/"))
        };
        let response = futures::executor::block_on(spawn_send(settings_client(|_| {}), request));
        assert_eq!(response.unwrap().unwrap().status, 200);

        let received = server.join().unwrap();
        let lower = received.to_ascii_lowercase();
        assert!(
            lower.contains("content-type: multipart/form-data; boundary="),
            "{received}"
        );
        assert!(!lower.contains("text/plain"), "{received}");
        assert!(received.contains("name=\"a\"\r\n\r\n1\r\n"), "{received}");
        assert!(received.contains("name=\"b\"\r\n\r\né\r\n"), "{received}");
        assert!(!received.contains("name=\"off\""), "{received}");
        assert!(!received.contains("name=\"\""), "{received}");
    }

    fn get(url: String) -> Request {
        Request {
            method: HttpMethod::Get,
            url,
            params: vec![],
            headers: vec![],
            body: Body::None,
            auth: Auth::None,
        }
    }

    fn send_blocking(client: reqwest::Client, url: String) -> Result<Response, SendError> {
        futures::executor::block_on(spawn_send(client, get(url))).unwrap()
    }

    fn default_config() -> ClientConfig {
        ClientConfig::from_settings(&RequestSettings::default()).unwrap()
    }

    fn client_with_timeout(timeout: Duration) -> reqwest::Client {
        client(&ClientConfig {
            timeout: Some(timeout),
            proxy: ProxyConfig::Off,
            ..default_config()
        })
        .unwrap()
    }

    fn app_client() -> reqwest::Client {
        client_with_timeout(Duration::from_secs(5))
    }

    /// A loopback server that answers one connection with `reply`, or holds it open when `None`.
    fn serve_once(reply: Option<&'static [u8]>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 1024];
            let _ = stream.read(&mut buf);
            match reply {
                Some(reply) => {
                    let _ = stream.write_all(reply);
                }
                None => thread::sleep(Duration::from_secs(5)),
            }
        });
        addr
    }

    #[test]
    fn https_to_a_plain_http_server_hints_at_http() {
        let addr = serve_once(Some(
            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
        ));
        let error = send_blocking(app_client(), format!("https://{addr}/")).unwrap_err();
        assert!(error.message.contains("InvalidContentType"), "{error:?}");
        assert_eq!(error.hint.as_deref(), Some(send_error::NOT_TLS_HINT));
    }

    #[test]
    fn plain_http_to_the_same_server_succeeds() {
        let addr = serve_once(Some(
            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
        ));
        let response = send_blocking(app_client(), format!("http://{addr}/")).unwrap();
        assert_eq!(response.status, 200);
    }

    #[test]
    fn refused_connection_hints_at_the_server() {
        let addr = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let error = send_blocking(app_client(), format!("http://{addr}/")).unwrap_err();
        assert_eq!(error.hint.as_deref(), Some(send_error::REFUSED_HINT));
    }

    #[test]
    fn timeout_hints_at_the_wait() {
        let addr = serve_once(None);
        let client = client_with_timeout(Duration::from_millis(200));
        let error = send_blocking(client, format!("http://{addr}/")).unwrap_err();
        assert_eq!(error.hint.as_deref(), Some(send_error::TIMEOUT_HINT));
    }

    #[test]
    fn aborted_send_stops_and_closes_the_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (accepted_tx, accepted) = mpsc::channel();
        let (closed_tx, closed) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            accepted_tx.send(()).unwrap();
            let mut buf = [0; 1024];
            while let Ok(1..) = stream.read(&mut buf) {}
            let _ = closed_tx.send(());
        });

        let client = client_with_timeout(Duration::from_secs(60));
        let handle = spawn_send(client, get(format!("http://{addr}/")));
        accepted.recv_timeout(Duration::from_secs(5)).unwrap();

        let start = Instant::now();
        handle.abort();
        let joined = futures::executor::block_on(handle);
        assert!(joined.unwrap_err().is_cancelled());
        assert!(start.elapsed() < Duration::from_secs(1));
        closed.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    struct FailingResolver;

    impl reqwest::dns::Resolve for FailingResolver {
        fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
            Box::pin(async { Err("no such host".into()) })
        }
    }

    #[test]
    fn failed_lookup_hints_at_the_host() {
        let client = reqwest::Client::builder()
            .dns_resolver(FailingResolver)
            .build()
            .unwrap();
        let error = send_blocking(client, "http://example.invalid/".into()).unwrap_err();
        assert!(error.message.contains("no such host"), "{error:?}");
        assert_eq!(error.hint.as_deref(), Some(send_error::DNS_HINT));
    }

    #[test]
    fn invalid_header_has_no_hint() {
        let request = Request {
            headers: vec![header("bad header", "v", true)],
            ..get("http://127.0.0.1:1/".into())
        };
        let error = futures::executor::block_on(spawn_send(app_client(), request))
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error.message,
            "invalid header name: \"bad header\": invalid HTTP header name"
        );
        assert_eq!(error.hint, None);
    }

    fn requests(f: impl FnOnce(&mut RequestSettings)) -> RequestSettings {
        let mut settings = RequestSettings::default();
        f(&mut settings);
        settings
    }

    fn config(f: impl FnOnce(&mut RequestSettings)) -> Result<ClientConfig, ConfigError> {
        ClientConfig::from_settings(&requests(f))
    }

    #[test]
    fn default_config_matches_the_defaults() {
        let config = default_config();
        assert_eq!(config.timeout, Some(Duration::from_secs(30)));
        assert_eq!(config.max_redirects, Some(10));
        assert!(config.verify_tls);
        assert_eq!(config.proxy, ProxyConfig::FromEnv);
        assert_eq!(config.user_agent, None);
    }

    #[test]
    fn zero_timeout_means_none() {
        assert_eq!(config(|s| s.timeout_secs = 0).unwrap().timeout, None);
        assert_eq!(
            config(|s| s.timeout_secs = 90).unwrap().timeout,
            Some(Duration::from_secs(90))
        );
    }

    #[test]
    fn redirects_follow_the_switch_and_stay_in_range() {
        let off = config(|s| {
            s.follow_redirects = false;
            s.max_redirects = 3;
        });
        assert_eq!(off.unwrap().max_redirects, None);
        assert_eq!(
            config(|s| s.max_redirects = 3).unwrap().max_redirects,
            Some(3)
        );
        assert_eq!(
            config(|s| s.max_redirects = 0).unwrap().max_redirects,
            Some(1)
        );
        assert_eq!(
            config(|s| s.max_redirects = 500).unwrap().max_redirects,
            Some(50)
        );
    }

    #[test]
    fn tls_verification_follows_the_setting() {
        assert!(!config(|s| s.verify_tls = false).unwrap().verify_tls);
    }

    #[test]
    fn proxy_modes() {
        let none = config(|s| {
            s.proxy = ProxyMode::None;
            s.proxy_url = "not used".into();
        });
        assert_eq!(none.unwrap().proxy, ProxyConfig::Off);
        let custom = config(|s| {
            s.proxy = ProxyMode::Custom;
            s.proxy_url = " http://proxy.local:3128 ".into();
        });
        assert_eq!(
            custom.unwrap().proxy,
            ProxyConfig::Url("http://proxy.local:3128".parse().unwrap())
        );
    }

    #[test]
    fn invalid_custom_proxy_is_an_error_only_when_used() {
        let invalid = |proxy| {
            config(|s| {
                s.proxy = proxy;
                s.proxy_url = "http://".into();
            })
        };
        assert!(matches!(
            invalid(ProxyMode::Custom),
            Err(ConfigError::ProxyUrl(ProxyUrlError::Invalid(_)))
        ));
        assert!(invalid(ProxyMode::System).is_ok());
        assert!(invalid(ProxyMode::None).is_ok());
    }

    #[test]
    fn proxy_url_parsing() {
        let parsed = |text| parse_proxy_url(text).map(|url| url.to_string());
        assert_eq!(
            parsed("proxy.local:3128").as_deref(),
            Ok("http://proxy.local:3128/")
        );
        assert_eq!(
            parsed("https://user:pw@proxy.local").as_deref(),
            Ok("https://user:pw@proxy.local/")
        );
        assert_eq!(parsed("   "), Err(ProxyUrlError::Empty));
        assert_eq!(
            parsed("socks5://proxy.local:1080"),
            Err(ProxyUrlError::UnsupportedScheme("socks5".into()))
        );
        assert!(matches!(
            parsed("http://proxy.local:99999"),
            Err(ProxyUrlError::Invalid(_))
        ));
        assert!(matches!(parsed("http://"), Err(ProxyUrlError::Invalid(_))));
    }

    #[test]
    fn proxy_errors_read_as_sentences() {
        assert_eq!(ProxyUrlError::Empty.to_string(), "Enter the proxy's URL.");
        assert_eq!(
            ProxyUrlError::UnsupportedScheme("socks5".into()).to_string(),
            "socks5:// proxies aren't supported. Use http:// or https://."
        );
        assert_eq!(
            parse_proxy_url("http://proxy.local:99999")
                .unwrap_err()
                .to_string(),
            "This isn't a valid URL (invalid port number)."
        );
    }

    #[test]
    fn user_agent_is_trimmed_and_checked() {
        assert_eq!(
            config(|s| s.user_agent = "  ".into()).unwrap().user_agent,
            None
        );
        assert_eq!(
            config(|s| s.user_agent = " pete/1 ".into())
                .unwrap()
                .user_agent,
            Some(HeaderValue::from_static("pete/1"))
        );
        assert_eq!(
            config(|s| s.user_agent = "a\u{7}b".into()),
            Err(ConfigError::UserAgent)
        );
    }

    #[test]
    fn every_valid_config_builds_a_client() {
        let settings = [
            requests(|_| {}),
            requests(|s| {
                s.timeout_secs = 0;
                s.follow_redirects = false;
                s.verify_tls = false;
                s.proxy = ProxyMode::None;
                s.user_agent = "pete/1".into();
            }),
            requests(|s| {
                s.proxy = ProxyMode::Custom;
                s.proxy_url = "https://proxy.local:8443".into();
            }),
        ];
        for settings in settings {
            let config = ClientConfig::from_settings(&settings).unwrap();
            client(&config).unwrap();
        }
    }

    /// A loopback server that answers `replies.len()` connections in order,
    /// sending each request's head on the returned channel.
    fn serve(replies: Vec<String>) -> (SocketAddr, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (addr, rx)
    }

    fn redirect_to(location: &str) -> String {
        format!(
            "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        )
    }

    const OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";

    fn settings_client(f: impl FnOnce(&mut RequestSettings)) -> reqwest::Client {
        let config = config(|s| {
            s.timeout_secs = 5;
            s.proxy = ProxyMode::None;
            f(s);
        });
        client(&config.unwrap()).unwrap()
    }

    #[test]
    fn redirects_are_followed_up_to_the_limit() {
        let (addr, _) = serve(vec![redirect_to("/a"), redirect_to("/b"), OK.into()]);
        let followed = send_blocking(settings_client(|_| {}), format!("http://{addr}/"));
        assert_eq!(followed.unwrap().status, 200);

        let (addr, _) = serve(vec![redirect_to("/a"), redirect_to("/b")]);
        let limited = settings_client(|s| s.max_redirects = 1);
        let error = send_blocking(limited, format!("http://{addr}/")).unwrap_err();
        assert!(error.message.contains("redirect"), "{error:?}");
    }

    #[test]
    fn redirects_are_returned_when_not_followed() {
        let (addr, _) = serve(vec![redirect_to("/a")]);
        let client = settings_client(|s| s.follow_redirects = false);
        let response = send_blocking(client, format!("http://{addr}/")).unwrap();
        assert_eq!(response.status, 302);
    }

    #[test]
    fn default_user_agent_yields_to_the_request_header() {
        let (addr, heads) = serve(vec![OK.into(), OK.into()]);
        let client = settings_client(|s| s.user_agent = "pete/1".into());
        send_blocking(client.clone(), format!("http://{addr}/")).unwrap();
        let head = heads.recv().unwrap().to_ascii_lowercase();
        assert!(head.contains("user-agent: pete/1\r\n"), "{head}");

        let request = Request {
            headers: vec![header("User-Agent", "mine", true)],
            ..get(format!("http://{addr}/"))
        };
        futures::executor::block_on(spawn_send(client, request))
            .unwrap()
            .unwrap();
        let head = heads.recv().unwrap().to_ascii_lowercase();
        assert!(head.contains("user-agent: mine\r\n"), "{head}");
        assert!(!head.contains("pete/1"), "{head}");
    }

    #[test]
    fn no_user_agent_by_default() {
        let (addr, heads) = serve(vec![OK.into()]);
        send_blocking(settings_client(|_| {}), format!("http://{addr}/")).unwrap();
        let head = heads.recv().unwrap().to_ascii_lowercase();
        assert!(!head.contains("user-agent"), "{head}");
    }

    #[test]
    fn custom_proxy_receives_the_request() {
        let (proxy, heads) = serve(vec![OK.into()]);
        let client = settings_client(|s| {
            s.proxy = ProxyMode::Custom;
            s.proxy_url = proxy.to_string();
        });
        let response = send_blocking(client, "http://example.invalid/path".into()).unwrap();
        assert_eq!(response.status, 200);
        let head = heads.recv().unwrap();
        assert!(
            head.starts_with("GET http://example.invalid/path HTTP/1.1\r\n"),
            "{head}"
        );
    }

    #[test]
    fn settings_client_refuses_while_invalid_and_recovers() {
        let mut settings = RequestSettings::default();
        let mut client = SettingsClient::new(&settings);
        assert!(client.get().is_ok());

        settings.proxy = ProxyMode::Custom;
        settings.proxy_url = "http://".into();
        client.apply(&settings);
        let error = client.get().unwrap_err().to_string();
        assert!(error.ends_with("Fix it in Settings › Requests."), "{error}");
        assert_eq!(client.config().unwrap().proxy, ProxyConfig::FromEnv);

        settings.proxy_url = "http://proxy.local:3128".into();
        client.apply(&settings);
        assert!(client.get().is_ok());
        assert_eq!(
            client.config().unwrap().proxy,
            ProxyConfig::Url("http://proxy.local:3128".parse().unwrap())
        );
    }

    #[test]
    fn settings_client_rebuilds_only_when_the_config_changes() {
        let mut settings = RequestSettings::default();
        let mut client = SettingsClient::new(&settings);
        assert_eq!(client.builds, 1);

        settings.proxy_url = "typing while not custom".into();
        client.apply(&settings);
        assert_eq!(client.builds, 1);

        settings.follow_redirects = false;
        client.apply(&settings);
        assert_eq!(client.builds, 2);

        settings.max_redirects = 3;
        client.apply(&settings);
        assert_eq!(client.builds, 2);

        settings.proxy = ProxyMode::Custom;
        client.apply(&settings);
        assert!(client.get().is_err());
        settings.proxy = ProxyMode::None;
        client.apply(&settings);
        settings.proxy = ProxyMode::System;
        client.apply(&settings);
        assert_eq!(client.builds, 4);
        assert!(client.get().is_ok());
    }
}
