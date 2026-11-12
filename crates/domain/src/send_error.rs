use std::{error::Error as StdError, fmt, io};

/// Why a request got no response: the error chain as text, with advice when
/// the cause is recognizable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendError {
    pub message: String,
    pub hint: Option<String>,
}

impl SendError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: None,
        }
    }

    pub fn from_error(error: &anyhow::Error) -> Self {
        Self {
            message: format!("{error:#}"),
            hint: hint(error).map(str::to_owned),
        }
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub const NOT_TLS_HINT: &str = "The server didn't respond with TLS. Did you mean http://?";
pub const REFUSED_HINT: &str =
    "Nothing accepted the connection. Check the host and port, and that the server is running.";
pub const DNS_HINT: &str =
    "The host name couldn't be found. Check the URL for typos and your network connection.";
pub const TIMEOUT_HINT: &str = "The server didn't respond in time.";

fn hint(error: &anyhow::Error) -> Option<&'static str> {
    let reqwest = || causes(error).filter_map(|e| e.downcast_ref::<reqwest::Error>());
    if causes(error).any(is_not_tls) {
        Some(NOT_TLS_HINT)
    } else if reqwest().any(reqwest::Error::is_timeout) {
        Some(TIMEOUT_HINT)
    } else if reqwest().any(reqwest::Error::is_dns) {
        Some(DNS_HINT)
    } else if causes(error).any(|e| io_kind(e) == Some(io::ErrorKind::ConnectionRefused)) {
        Some(REFUSED_HINT)
    } else {
        None
    }
}

/// The error and its causes. Unlike `source()`, this steps into the error an
/// `io::Error` wraps (rustls errors arrive inside nested `io::Error`s).
fn causes(error: &anyhow::Error) -> impl Iterator<Item = &(dyn StdError + 'static)> {
    let mut next: Option<&(dyn StdError + 'static)> = Some(&**error);
    std::iter::from_fn(move || {
        let current = next?;
        next = current
            .downcast_ref::<io::Error>()
            .and_then(io::Error::get_ref)
            .map(|inner| inner as &(dyn StdError + 'static))
            .or_else(|| current.source());
        Some(current)
    })
}

fn io_kind(error: &(dyn StdError + 'static)) -> Option<io::ErrorKind> {
    error.downcast_ref::<io::Error>().map(io::Error::kind)
}

/// rustls' `InvalidMessage(InvalidContentType)`: the first bytes back weren't
/// a TLS record, as when a plain-HTTP server answers. rustls isn't a direct
/// dependency, so it's matched by its Debug text.
fn is_not_tls(error: &(dyn StdError + 'static)) -> bool {
    format!("{error:?}") == "InvalidMessage(InvalidContentType)"
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn message_is_the_whole_chain() {
        let error = anyhow::Error::new(io::Error::other("boom")).context("sending");
        let error = SendError::from_error(&error);
        assert_eq!(error.message, "sending: boom");
        assert_eq!(error.hint, None);
        assert_eq!(error.to_string(), "sending: boom");
    }

    #[test]
    fn finds_causes_inside_nested_io_errors() {
        let refused = io::Error::from(io::ErrorKind::ConnectionRefused);
        let wrapped = io::Error::other(io::Error::other(refused));
        let error = anyhow::Error::new(wrapped).context("connecting");
        assert_eq!(
            SendError::from_error(&error).hint.as_deref(),
            Some(REFUSED_HINT)
        );
    }

    #[test]
    fn other_io_errors_have_no_hint() {
        let error = anyhow::Error::new(io::Error::from(io::ErrorKind::ConnectionReset));
        assert_eq!(SendError::from_error(&error).hint, None);
    }
}
