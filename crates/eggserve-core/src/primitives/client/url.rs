//! URL parsing and validation for the HTTP client.
//!
//! Supports `http://` and `https://` schemes. No new dependency is introduced;
//! parsing is hand-rolled for the narrow set of URLs the client needs.

use std::fmt;

use super::error::ClientError;

/// Supported URL schemes for the HTTP client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parsed HTTP URL with validated components.
#[derive(Debug, Clone)]
pub struct ParsedUrl {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl ParsedUrl {
    /// Parse a URL string into validated components.
    ///
    /// Only `http://` and `https://` schemes are supported. The host is
    /// required. Port defaults to 80/443 based on scheme if not specified.
    /// The path always starts with `/`.
    pub fn parse(url: &str) -> Result<Self, ClientError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(ClientError::InvalidUrl("empty URL".into()));
        }

        // Find scheme
        let scheme_end = url.find("://").ok_or_else(|| {
            ClientError::InvalidUrl("missing scheme (expected http:// or https://)".into())
        })?;
        let scheme_str = &url[..scheme_end];
        let scheme = match scheme_str {
            "http" => Scheme::Http,
            "https" => Scheme::Https,
            other => return Err(ClientError::UnsupportedScheme(other.into())),
        };

        let rest = &url[scheme_end + 3..];

        // Reject userinfo (user:pass@host)
        if rest.contains('@') {
            return Err(ClientError::InvalidUrl(
                "userinfo (user:pass@) not supported".into(),
            ));
        }

        // Split path from host[:port]
        let (host_port, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };

        if host_port.is_empty() {
            return Err(ClientError::MissingHost);
        }

        // Handle IPv6 literals [::1]
        let (host, port) = if host_port.starts_with('[') {
            let bracket_end = host_port.find(']').ok_or_else(|| {
                ClientError::InvalidUrl("unclosed bracket in IPv6 literal".into())
            })?;
            let host = host_port[1..bracket_end].to_string();
            let after_bracket = &host_port[bracket_end + 1..];
            let port = if after_bracket.starts_with(':') {
                after_bracket[1..]
                    .parse::<u16>()
                    .map_err(|_| ClientError::InvalidUrl("invalid port number".into()))?
            } else {
                scheme.default_port()
            };
            (host, port)
        } else {
            // Regular host:port
            match host_port.rfind(':') {
                Some(colon) => {
                    let host = &host_port[..colon];
                    let port_str = &host_port[colon + 1..];
                    let port = port_str
                        .parse::<u16>()
                        .map_err(|_| ClientError::InvalidUrl("invalid port number".into()))?;
                    (host.to_string(), port)
                }
                None => (host_port.to_string(), scheme.default_port()),
            }
        };

        if host.is_empty() {
            return Err(ClientError::MissingHost);
        }

        // Validate host: only ASCII alphanumeric, hyphens, dots, colons (IPv6)
        if !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == ':')
        {
            return Err(ClientError::InvalidUrl(format!(
                "invalid characters in host: {host}"
            )));
        }

        // Normalize path: ensure it starts with /
        let path = if path.is_empty() {
            "/".to_string()
        } else if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        Ok(ParsedUrl {
            scheme,
            host,
            port,
            path,
        })
    }

    /// Returns the authority (host:port) string, omitting the port if it
    /// matches the scheme default.
    pub fn authority(&self) -> String {
        let default_port = self.scheme.default_port();
        if self.port == default_port {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// Returns the full URL string.
    pub fn as_url(&self) -> String {
        format!("{}://{}{}", self.scheme, self.authority(), self.path)
    }

    /// Returns true if the scheme is HTTPS.
    pub fn is_https(&self) -> bool {
        self.scheme == Scheme::Https
    }
}

impl fmt::Display for ParsedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_url() {
        let url = ParsedUrl::parse("http://example.com/").unwrap();
        assert_eq!(url.scheme, Scheme::Http);
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/");
    }

    #[test]
    fn parse_https_url() {
        let url = ParsedUrl::parse("https://example.com/").unwrap();
        assert_eq!(url.scheme, Scheme::Https);
        assert_eq!(url.port, 443);
    }

    #[test]
    fn parse_with_custom_port() {
        let url = ParsedUrl::parse("http://localhost:8080/api").unwrap();
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 8080);
        assert_eq!(url.path, "/api");
    }

    #[test]
    fn parse_with_path() {
        let url = ParsedUrl::parse("http://example.com/foo/bar?q=1").unwrap();
        assert_eq!(url.path, "/foo/bar?q=1");
    }

    #[test]
    fn parse_no_path_defaults_to_slash() {
        let url = ParsedUrl::parse("http://example.com").unwrap();
        assert_eq!(url.path, "/");
    }

    #[test]
    fn parse_ipv6_literal() {
        let url = ParsedUrl::parse("http://[::1]:8080/").unwrap();
        assert_eq!(url.host, "::1");
        assert_eq!(url.port, 8080);
    }

    #[test]
    fn parse_ipv6_no_port() {
        let url = ParsedUrl::parse("http://[::1]/").unwrap();
        assert_eq!(url.host, "::1");
        assert_eq!(url.port, 80);
    }

    #[test]
    fn parse_empty_url_rejected() {
        assert!(ParsedUrl::parse("").is_err());
    }

    #[test]
    fn parse_no_scheme_rejected() {
        assert!(ParsedUrl::parse("example.com/").is_err());
    }

    #[test]
    fn parse_unsupported_scheme_rejected() {
        let err = ParsedUrl::parse("ftp://example.com/").unwrap_err();
        assert!(matches!(err, ClientError::UnsupportedScheme(_)));
    }

    #[test]
    fn parse_userinfo_rejected() {
        let err = ParsedUrl::parse("http://user:pass@example.com/").unwrap_err();
        assert!(matches!(err, ClientError::InvalidUrl(_)));
    }

    #[test]
    fn parse_empty_host_rejected() {
        assert!(ParsedUrl::parse("http:///path").is_err());
    }

    #[test]
    fn parse_invalid_port_rejected() {
        assert!(ParsedUrl::parse("http://example.com:99999/").is_err());
    }

    #[test]
    fn parse_unclosed_ipv6_bracket_rejected() {
        assert!(ParsedUrl::parse("http://[::1/").is_err());
    }

    #[test]
    fn authority_omits_default_port() {
        let url = ParsedUrl::parse("http://example.com/").unwrap();
        assert_eq!(url.authority(), "example.com");
    }

    #[test]
    fn authority_includes_non_default_port() {
        let url = ParsedUrl::parse("http://example.com:8080/").unwrap();
        assert_eq!(url.authority(), "example.com:8080");
    }

    #[test]
    fn authority_includes_non_default_https_port() {
        let url = ParsedUrl::parse("https://example.com:8443/").unwrap();
        assert_eq!(url.authority(), "example.com:8443");
    }

    #[test]
    fn display_roundtrips() {
        let url = ParsedUrl::parse("http://localhost:3000/api").unwrap();
        assert_eq!(url.to_string(), "http://localhost:3000/api");
    }

    #[test]
    fn is_https_true_for_https() {
        let url = ParsedUrl::parse("https://example.com/").unwrap();
        assert!(url.is_https());
    }

    #[test]
    fn is_https_false_for_http() {
        let url = ParsedUrl::parse("http://example.com/").unwrap();
        assert!(!url.is_https());
    }
}
