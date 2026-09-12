//! Canonical effective request authority.
//!
//! Protocol adapters map HTTP/1 `Host` and HTTP/2/3 `:authority` into this
//! value.  Services never need to inspect pseudo-header names, and forwarded
//! headers remain ordinary untrusted headers.

use std::fmt;

/// A validated host authority with an optional numeric port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Authority(String);

/// Authority validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityError {
    /// The authority is empty or contains invalid syntax.
    Invalid,
}

impl fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid request authority")
    }
}

impl std::error::Error for AuthorityError {}

impl Authority {
    /// Parse a host authority such as `example.test:443` or `[::1]:8443`.
    ///
    /// The stored value is the validated, OWS-trimmed textual authority. Host
    /// names are intentionally ASCII-only; IDNA conversion belongs to a
    /// protocol adapter or application policy and is never performed here.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, AuthorityError> {
        let value = value.as_ref().trim_matches(|c| c == ' ' || c == '\t');
        if value.is_empty()
            || !value.is_ascii()
            || value
                .bytes()
                .any(|b| b.is_ascii_control() || matches!(b, b'/' | b'?' | b'#' | b'\\' | b'@'))
        {
            return Err(AuthorityError::Invalid);
        }

        let (host, port) = if value.starts_with('[') {
            let close = value.find(']').ok_or(AuthorityError::Invalid)?;
            let host = &value[1..close];
            if host.is_empty()
                || host.contains('[')
                || host.contains(']')
                || !host.bytes().all(
                    |b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b':' | b'.' | b'%'),
                )
            {
                return Err(AuthorityError::Invalid);
            }
            let suffix = &value[close + 1..];
            let port = suffix.strip_prefix(':').map(parse_port).transpose()?;
            if !suffix.is_empty() && !suffix.starts_with(':') {
                return Err(AuthorityError::Invalid);
            }
            (host, port)
        } else {
            let mut split = value.split(':');
            let host = split.next().unwrap_or_default();
            let port = split.next().map(parse_port).transpose()?;
            if split.next().is_some()
                || host.is_empty()
                || !host.bytes().all(|b| {
                    matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b'%')
                })
            {
                return Err(AuthorityError::Invalid);
            }
            (host, port)
        };

        let _ = (host, port);
        Ok(Self(value.to_owned()))
    }

    /// Returns the canonical authority text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the authority bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Display for Authority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn parse_port(value: &str) -> Result<u16, AuthorityError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AuthorityError::Invalid);
    }
    value.parse().map_err(|_| AuthorityError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_host_port_and_ipv6() {
        assert_eq!(
            Authority::parse("example.test:443").unwrap().as_str(),
            "example.test:443"
        );
        assert_eq!(
            Authority::parse("[::1]:8443").unwrap().as_str(),
            "[::1]:8443"
        );
        assert!(Authority::parse("[2001:db8::1]").is_ok());
    }

    #[test]
    fn rejects_ambiguous_or_invalid_authority() {
        for value in [
            "",
            ":443",
            "example.test:",
            "2001:db8::1",
            "user@example.test",
            "host:65536",
        ] {
            assert!(Authority::parse(value).is_err(), "accepted {value:?}");
        }
    }
}
