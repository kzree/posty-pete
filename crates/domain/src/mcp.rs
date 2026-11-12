use std::{collections::BTreeMap, ops::RangeInclusive};

use serde::Serialize;

pub const DEFAULT_PORT: u16 = 7457;
pub const PORT_RANGE: RangeInclusive<u16> = 1024..=65535;
/// Shown until real tokens exist; not a secret.
pub const PLACEHOLDER_TOKEN: &str = "pp_••••••••";
const SERVER_NAME: &str = "posty-pete";

/// The port in `text`, if it's a whole number within [`PORT_RANGE`].
pub fn parse_port(text: &str) -> Option<u16> {
    text.trim()
        .parse()
        .ok()
        .filter(|port| PORT_RANGE.contains(port))
}

/// The local address the server listens on.
pub fn server_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientConfig<'a> {
    mcp_servers: BTreeMap<&'a str, ServerEntry>,
}

#[derive(Serialize)]
struct ServerEntry {
    #[serde(rename = "type")]
    kind: &'static str,
    url: String,
    headers: Headers,
}

#[derive(Serialize)]
struct Headers {
    #[serde(rename = "Authorization")]
    authorization: String,
}

/// An MCP client configuration (`.mcp.json` style) pointing at the server.
pub fn client_config(port: u16, token: &str) -> String {
    let entry = ServerEntry {
        kind: "http",
        url: server_url(port),
        headers: Headers {
            authorization: format!("Bearer {token}"),
        },
    };
    let config = ClientConfig {
        mcp_servers: BTreeMap::from([(SERVER_NAME, entry)]),
    };
    serde_json::to_string_pretty(&config).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_uses_loopback_and_port() {
        assert_eq!(server_url(7457), "http://127.0.0.1:7457/mcp");
        assert_eq!(server_url(8080), "http://127.0.0.1:8080/mcp");
    }

    #[test]
    fn parses_ports_in_range() {
        assert_eq!(parse_port("7457"), Some(7457));
        assert_eq!(parse_port(" 1024 "), Some(1024));
        assert_eq!(parse_port("65535"), Some(65535));
        assert_eq!(parse_port("1023"), None);
        assert_eq!(parse_port("65536"), None);
        assert_eq!(parse_port(""), None);
        assert_eq!(parse_port("80a"), None);
    }

    #[test]
    fn default_port_is_in_range() {
        assert!(PORT_RANGE.contains(&DEFAULT_PORT));
    }

    #[test]
    fn client_config_has_url_and_token() {
        let expected = r#"{
  "mcpServers": {
    "posty-pete": {
      "type": "http",
      "url": "http://127.0.0.1:9000/mcp",
      "headers": {
        "Authorization": "Bearer pp_test"
      }
    }
  }
}"#;
        assert_eq!(client_config(9000, "pp_test"), expected);
    }
}
