use std::{borrow::Cow, net::IpAddr};

/// Adds a scheme when the URL has none: `http://` for local addresses, `https://` otherwise.
pub fn with_default_scheme(input: &str) -> Cow<'_, str> {
    let input = input.trim();
    if input.contains("://") {
        return Cow::Borrowed(input);
    }
    let scheme = if is_local(authority(input)) { "http" } else { "https" };
    Cow::Owned(format!("{scheme}://{input}"))
}

fn authority(input: &str) -> &str {
    input.split(['/', '?', '#']).next().unwrap_or_default()
}

fn is_local(authority: &str) -> bool {
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    // IPv6 hosts are bracketed: `[::1]:8080`.
    let host = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => host.rsplit_once(':').map_or(host, |(host, _)| host),
    };

    let host = host.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_unspecified(),
        Ok(IpAddr::V6(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::with_default_scheme;

    #[test]
    fn keeps_existing_scheme() {
        assert_eq!(with_default_scheme("https://localhost:4000"), "https://localhost:4000");
        assert_eq!(with_default_scheme("http://example.com"), "http://example.com");
    }

    #[test]
    fn local_hosts_get_http() {
        for input in [
            "localhost:4000/health",
            "localhost",
            "api.localhost/v1",
            "127.0.0.1:8080",
            "192.168.1.20/status",
            "10.0.0.5",
            "0.0.0.0:3000",
            "[::1]:8080/x",
            "user:pass@localhost:4000",
        ] {
            assert_eq!(with_default_scheme(input), format!("http://{input}"), "{input}");
        }
    }

    #[test]
    fn other_hosts_get_https() {
        for input in ["example.com", "httpbin.org/get?x=1", "8.8.8.8", "localhost.example.com"] {
            assert_eq!(with_default_scheme(input), format!("https://{input}"), "{input}");
        }
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(with_default_scheme("  localhost:4000 "), "http://localhost:4000");
    }
}
