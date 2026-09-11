//! Trusted proxy metadata and HAProxy PROXY protocol (Plan 202).
//!
//! Forwarding metadata is untrusted by default. EggServe never reinterprets
//! `Forwarded`, `X-Forwarded-*`, or a PROXY preamble merely because it is
//! present. Trust is configured against the immediate peer/listener boundary.
//!
//! EggServe does not become a reverse proxy in this plan.
//!
//! # Layers
//!
//! - Raw transport peer/local endpoints are always preserved in
//!   [`crate::primitives::connection_info::ConnectionInfo`].
//! - An optional PROXY preamble (v1/v2) parsed before TLS/HTTP populates a
//!   provenance-tagged proxy layer; it never replaces raw endpoints.
//! - An optional header-derived layer (`Forwarded` / `X-Forwarded-*`) is
//!   derived per request only when the immediate peer is explicitly trusted.
//!   The canonical `Host`/request-target is never silently rewritten; trusted
//!   values populate effective fields the downstream application may choose
//!   to use.
//!
//! # Single trusted hop
//!
//! The first implementation supports the common single trusted hop:
//! the immediate peer is trusted, and the rightmost chain element (closest to
//! the trusted proxy) becomes the effective value. Multi-proxy chains beyond
//! one trusted hop remain untrusted by documentation; a future plan may add
//! an explicit hop budget.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use crate::primitives::authority::Authority;
use crate::primitives::connection_info::Scheme;
use crate::primitives::header_block::HeaderBlock;

// ---------------------------------------------------------------------------
// Constants (bounded parsing)
// ---------------------------------------------------------------------------

/// Maximum PROXY v1 line length including CRLF (spec: 107 bytes).
pub const PROXY_V1_MAX_BYTES: usize = 107;
/// PROXY v2 signature (12 bytes).
pub const PROXY_V2_SIGNATURE: [u8; 12] = [
    0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
];
/// PROXY v2 fixed header length (signature + ver/cmd + fam/proto + len).
pub const PROXY_V2_HEADER_LEN: usize = 16;
/// Maximum PROXY v2 remaining length (addresses + TLVs) we will buffer.
/// Addresses need at most 216 bytes (UNIX); TLVs are ignored but bounded.
pub const PROXY_V2_MAX_LEN: usize = 1024;
/// Default PROXY preamble read timeout.
pub const DEFAULT_PROXY_PROTOCOL_TIMEOUT: Duration = Duration::from_secs(5);
/// Default maximum total forwarded-header bytes examined per request.
pub const DEFAULT_FORWARDED_MAX_BYTES: usize = 4096;
/// Minimum forwarded-header byte budget.
pub const MIN_FORWARDED_MAX_BYTES: usize = 256;
/// Maximum forwarded-header byte budget.
pub const MAX_FORWARDED_MAX_BYTES: usize = 16384;
/// Default maximum forwarded chain elements examined per request.
pub const DEFAULT_FORWARDED_MAX_ELEMENTS: usize = 16;
/// Minimum forwarded chain elements.
pub const MIN_FORWARDED_MAX_ELEMENTS: usize = 1;
/// Maximum forwarded chain elements.
pub const MAX_FORWARDED_MAX_ELEMENTS: usize = 64;

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Where trusted effective metadata came from.
///
/// Used for observability (`proxy_v1`, `proxy_v2`, `forwarded`,
/// `legacy_forwarded`) and to audit which trust path populated an effective
/// value. Untrusted requests carry `None` provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProxySourceKind {
    /// HAProxy PROXY protocol version 1 (text line).
    ProxyV1,
    /// HAProxy PROXY protocol version 2 (binary).
    ProxyV2,
    /// Standardized `Forwarded` header (RFC 7239).
    Forwarded,
    /// Legacy `X-Forwarded-For` / `X-Forwarded-Proto` / `X-Forwarded-Host`.
    LegacyForwarded,
}

impl ProxySourceKind {
    /// Short stable identifier for logs/metrics.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProxyV1 => "proxy_v1",
            Self::ProxyV2 => "proxy_v2",
            Self::Forwarded => "forwarded",
            Self::LegacyForwarded => "legacy_forwarded",
        }
    }
}

impl fmt::Display for ProxySourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Trusted peer policy (Track B)
// ---------------------------------------------------------------------------

/// An exact IP or CIDR prefix trusted to supply proxy metadata.
///
/// Examples: `192.168.0.10` (exact), `192.168.0.0/24`, `2001:db8::/32`,
/// `::1` (loopback must be listed explicitly; it is never implicitly trusted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IpPrefix {
    network: IpAddr,
    prefix_len: u8,
}

impl IpPrefix {
    /// Parse an exact IP (`192.168.0.10`) or CIDR (`192.168.0.0/24`).
    ///
    /// DNS names are never accepted. Prefix lengths must be in range
    /// (`0..=32` for IPv4, `0..=128` for IPv6).
    pub fn parse(value: &str) -> Result<Self, TrustedProxyConfigError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(TrustedProxyConfigError::InvalidPeer(
                "trusted peer is empty".into(),
            ));
        }
        if let Some((addr_part, prefix_part)) = value.split_once('/') {
            let addr: IpAddr = addr_part.trim().parse().map_err(|_| {
                TrustedProxyConfigError::InvalidPeer(format!(
                    "invalid IP in trusted peer {value:?}"
                ))
            })?;
            let prefix_len: u8 = prefix_part.trim().parse().map_err(|_| {
                TrustedProxyConfigError::InvalidPeer(format!(
                    "invalid prefix length in trusted peer {value:?}"
                ))
            })?;
            let max = match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            if prefix_len > max {
                return Err(TrustedProxyConfigError::InvalidPeer(format!(
                    "prefix length {prefix_len} out of range for trusted peer {value:?}"
                )));
            }
            Ok(Self {
                network: mask_addr(&addr, prefix_len),
                prefix_len,
            })
        } else {
            let addr: IpAddr = value.parse().map_err(|_| {
                TrustedProxyConfigError::InvalidPeer(format!(
                    "invalid IP in trusted peer {value:?}; DNS names are never trusted"
                ))
            })?;
            let prefix_len = match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            Ok(Self {
                network: addr,
                prefix_len,
            })
        }
    }

    /// Returns `true` when `ip` is within this prefix (same family only).
    pub fn matches(&self, ip: &IpAddr) -> bool {
        match (&self.network, ip) {
            (IpAddr::V4(net), IpAddr::V4(addr)) => {
                let net_u32 = u32::from_be_bytes(net.octets());
                let addr_u32 = u32::from_be_bytes(addr.octets());
                if self.prefix_len == 0 {
                    return true;
                }
                let shift = 32 - self.prefix_len;
                (net_u32 >> shift) == (addr_u32 >> shift)
            }
            (IpAddr::V6(net), IpAddr::V6(addr)) => {
                let net_u128 = u128::from_be_bytes(net.octets());
                let addr_u128 = u128::from_be_bytes(addr.octets());
                if self.prefix_len == 0 {
                    return true;
                }
                let shift = 128 - self.prefix_len;
                (net_u128 >> shift) == (addr_u128 >> shift)
            }
            _ => false,
        }
    }

    /// The masked network address.
    pub fn network(&self) -> IpAddr {
        self.network
    }

    /// The prefix length.
    pub fn prefix_len(&self) -> u8 {
        self.prefix_len
    }
}

impl fmt::Display for IpPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let max = match self.network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if self.prefix_len == max {
            write!(f, "{}", self.network)
        } else {
            write!(f, "{}/{}", self.network, self.prefix_len)
        }
    }
}

fn mask_addr(addr: &IpAddr, prefix_len: u8) -> IpAddr {
    match addr {
        IpAddr::V4(v4) => {
            if prefix_len == 0 {
                IpAddr::V4(Ipv4Addr::UNSPECIFIED)
            } else {
                let bits = u32::from_be_bytes(v4.octets());
                let shift = 32 - prefix_len;
                let masked = (bits >> shift) << shift;
                IpAddr::V4(Ipv4Addr::from(masked))
            }
        }
        IpAddr::V6(v6) => {
            if prefix_len == 0 {
                IpAddr::V6(Ipv6Addr::UNSPECIFIED)
            } else {
                let bits = u128::from_be_bytes(v6.octets());
                let shift = 128 - prefix_len;
                let masked = (bits >> shift) << shift;
                IpAddr::V6(Ipv6Addr::from(masked))
            }
        }
    }
}

/// Configuration validation failure for trusted-proxy policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedProxyConfigError {
    /// A trusted peer entry is not an IP/CIDR.
    InvalidPeer(String),
    /// PROXY protocol configuration is invalid.
    InvalidProxyProtocol(String),
    /// Forwarded-header configuration is invalid.
    InvalidForwarded(String),
}

impl fmt::Display for TrustedProxyConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPeer(msg) => write!(f, "invalid trusted peer: {msg}"),
            Self::InvalidProxyProtocol(msg) => write!(f, "invalid proxy protocol config: {msg}"),
            Self::InvalidForwarded(msg) => write!(f, "invalid forwarded config: {msg}"),
        }
    }
}

impl std::error::Error for TrustedProxyConfigError {}

// ---------------------------------------------------------------------------
// PROXY protocol transport config (Track C)
// ---------------------------------------------------------------------------

/// Optional PROXY protocol preamble handling.
///
/// When disabled (default), listeners interpret bytes normally with no
/// magic auto-detection. When enabled, the listener reads a bounded preamble
/// before TLS/HTTP, only from explicitly trusted immediate peers; malformed
/// or untrusted input closes before TLS/HTTP and never reaches a service.
///
/// Order is always: `TCP accept -> PROXY preamble -> TLS (optional) -> HTTP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyProtocolConfig {
    /// Whether to require and parse a PROXY preamble.
    pub enabled: bool,
    /// Timeout for reading the complete preamble.
    pub timeout: Duration,
}

impl Default for ProxyProtocolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout: DEFAULT_PROXY_PROTOCOL_TIMEOUT,
        }
    }
}

impl ProxyProtocolConfig {
    /// Validate the config.
    pub fn validate(&self) -> Result<(), TrustedProxyConfigError> {
        if self.timeout.is_zero() {
            return Err(TrustedProxyConfigError::InvalidProxyProtocol(
                "proxy protocol timeout must be > 0".into(),
            ));
        }
        if self.timeout > Duration::from_secs(60) {
            return Err(TrustedProxyConfigError::InvalidProxyProtocol(
                "proxy protocol timeout must be <= 60s".into(),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Forwarded header policy (Track D)
// ---------------------------------------------------------------------------

/// Which header-derived forwarding signals are honored (only from trusted peers).
///
/// Both default to disabled. Enabling both with both header families present
/// on one request is a conflict: the request remains untrusted (fail closed)
/// rather than guessing precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForwardedConfig {
    /// Honor the standardized `Forwarded` header (RFC 7239).
    pub standard_enabled: bool,
    /// Honor legacy `X-Forwarded-For` / `X-Forwarded-Proto` / `X-Forwarded-Host`.
    pub legacy_enabled: bool,
    /// Maximum total forwarded-header bytes examined per request.
    pub max_bytes: usize,
    /// Maximum forwarded chain elements examined per request.
    pub max_elements: usize,
}

impl Default for ForwardedConfig {
    fn default() -> Self {
        Self {
            standard_enabled: false,
            legacy_enabled: false,
            max_bytes: DEFAULT_FORWARDED_MAX_BYTES,
            max_elements: DEFAULT_FORWARDED_MAX_ELEMENTS,
        }
    }
}

impl ForwardedConfig {
    /// Returns `true` when no header-derived metadata is honored.
    pub fn is_disabled(&self) -> bool {
        !self.standard_enabled && !self.legacy_enabled
    }

    /// Validate budgets.
    pub fn validate(&self) -> Result<(), TrustedProxyConfigError> {
        if self.max_bytes < MIN_FORWARDED_MAX_BYTES || self.max_bytes > MAX_FORWARDED_MAX_BYTES {
            return Err(TrustedProxyConfigError::InvalidForwarded(format!(
                "forwarded max_bytes must be between {MIN_FORWARDED_MAX_BYTES} and {MAX_FORWARDED_MAX_BYTES}"
            )));
        }
        if self.max_elements < MIN_FORWARDED_MAX_ELEMENTS
            || self.max_elements > MAX_FORWARDED_MAX_ELEMENTS
        {
            return Err(TrustedProxyConfigError::InvalidForwarded(format!(
                "forwarded max_elements must be between {MIN_FORWARDED_MAX_ELEMENTS} and {MAX_FORWARDED_MAX_ELEMENTS}"
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Combined trusted-proxy config (Track B)
// ---------------------------------------------------------------------------

/// Explicit policy identifying which immediate peers may supply trusted metadata.
///
/// Defaults trust nothing: no peer is trusted, loopback is not implicitly
/// trusted, Unix listeners are not implicitly trusted, PROXY parsing is
/// disabled, and header-derived forwarding is disabled.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustedProxyConfig {
    /// Exact IPs/CIDRs trusted as immediate peers. Empty means none trusted.
    pub peers: Vec<IpPrefix>,
    /// Whether Unix-domain listeners are explicitly trusted for
    /// header-derived forwarding. Never implicit. PROXY preambles are not
    /// read from Unix streams.
    pub trust_unix: bool,
    /// Optional PROXY protocol preamble handling.
    pub proxy_protocol: ProxyProtocolConfig,
    /// Optional header-derived forwarding policy.
    pub forwarded: ForwardedConfig,
}

impl TrustedProxyConfig {
    /// Returns `true` when `peer` is explicitly listed.
    pub fn is_trusted_peer(&self, peer: &SocketAddr) -> bool {
        let ip = peer.ip();
        self.peers.iter().any(|prefix| prefix.matches(&ip))
    }

    /// Parse and append one peer entry (`IP` or `IP/prefix`).
    pub fn push_peer(&mut self, value: &str) -> Result<(), TrustedProxyConfigError> {
        let prefix = IpPrefix::parse(value)?;
        self.peers.push(prefix);
        Ok(())
    }

    /// Validate the combined policy.
    pub fn validate(&self) -> Result<(), TrustedProxyConfigError> {
        self.proxy_protocol.validate()?;
        self.forwarded.validate()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PROXY endpoints + pure parsers (Track C)
// ---------------------------------------------------------------------------

/// Trusted proxy-reported endpoints from a PROXY preamble.
///
/// `source`/`destination` are `None` for `LOCAL`, `UNKNOWN`, `UNSPEC`, and
/// UNIX families (truthful absence; no identity is invented). Raw
/// peer/local endpoints are always preserved separately; these populate the
/// provenance-tagged effective layer only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyEndpoints {
    /// Proxy-reported source endpoint, when the preamble carries one.
    pub source: Option<SocketAddr>,
    /// Proxy-reported destination endpoint, when the preamble carries one.
    pub destination: Option<SocketAddr>,
    /// Preamble version that produced this value.
    pub kind: ProxySourceKind,
}

/// PROXY preamble parse failure (sanitized; no preamble bytes reflected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyParseError {
    /// Need more bytes (caller should keep reading within bounds/timeout).
    Incomplete,
    /// Preamble exceeds the strict size ceiling.
    TooLong,
    /// Preamble is malformed or uses an unsupported family/protocol.
    Invalid,
}

impl fmt::Display for ProxyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete => write!(f, "incomplete PROXY preamble"),
            Self::TooLong => write!(f, "PROXY preamble exceeds size limit"),
            Self::Invalid => write!(f, "invalid PROXY preamble"),
        }
    }
}

impl std::error::Error for ProxyParseError {}

/// Parse one complete PROXY v1 line (must include trailing CRLF).
///
/// Strict and bounded: max [`PROXY_V1_MAX_BYTES`] bytes, `TCP4`/`TCP6` with
/// exactly six space-separated parts, `UNKNOWN` preserves truthful absence.
/// `UNIX` and any other protocol are deterministically rejected.
pub fn parse_proxy_v1_line(line: &[u8]) -> Result<ProxyEndpoints, ProxyParseError> {
    if line.len() > PROXY_V1_MAX_BYTES {
        return Err(ProxyParseError::TooLong);
    }
    if line.len() < 8 || !line.ends_with(b"\r\n") {
        // Too short to be valid or missing terminator: the async reader
        // treats missing CRLF as incomplete; a direct call with no CRLF is
        // invalid input.
        return Err(ProxyParseError::Invalid);
    }
    if !line.starts_with(b"PROXY ") {
        return Err(ProxyParseError::Invalid);
    }
    // ASCII-only preamble; reject non-ASCII without reflecting bytes.
    if !line.is_ascii() {
        return Err(ProxyParseError::Invalid);
    }
    let inner = &line[..line.len() - 2];
    let text = std::str::from_utf8(inner).map_err(|_| ProxyParseError::Invalid)?;

    // UNKNOWN preserves truthful absence (ignore any trailing words).
    if text == "PROXY UNKNOWN" || text.starts_with("PROXY UNKNOWN ") {
        return Ok(ProxyEndpoints {
            source: None,
            destination: None,
            kind: ProxySourceKind::ProxyV1,
        });
    }

    // Strict six-part split with no empty parts (rejects double spaces).
    let parts: Vec<&str> = text.split(' ').collect();
    if parts.len() != 6 || parts.iter().any(|part| part.is_empty()) {
        return Err(ProxyParseError::Invalid);
    }
    if parts[0] != "PROXY" {
        return Err(ProxyParseError::Invalid);
    }
    let is_v4 = match parts[1] {
        "TCP4" => true,
        "TCP6" => false,
        _ => return Err(ProxyParseError::Invalid),
    };
    let src_ip: IpAddr = if is_v4 {
        parts[2]
            .parse::<Ipv4Addr>()
            .map(IpAddr::V4)
            .map_err(|_| ProxyParseError::Invalid)?
    } else {
        parts[2]
            .parse::<Ipv6Addr>()
            .map(IpAddr::V6)
            .map_err(|_| ProxyParseError::Invalid)?
    };
    let dst_ip: IpAddr = if is_v4 {
        parts[3]
            .parse::<Ipv4Addr>()
            .map(IpAddr::V4)
            .map_err(|_| ProxyParseError::Invalid)?
    } else {
        parts[3]
            .parse::<Ipv6Addr>()
            .map(IpAddr::V6)
            .map_err(|_| ProxyParseError::Invalid)?
    };
    // Families must agree with the declared protocol.
    match (&src_ip, &dst_ip, is_v4) {
        (IpAddr::V4(_), IpAddr::V4(_), true) => {}
        (IpAddr::V6(_), IpAddr::V6(_), false) => {}
        _ => return Err(ProxyParseError::Invalid),
    }
    let src_port = parse_port_strict(parts[4])?;
    let dst_port = parse_port_strict(parts[5])?;
    Ok(ProxyEndpoints {
        source: Some(SocketAddr::new(src_ip, src_port)),
        destination: Some(SocketAddr::new(dst_ip, dst_port)),
        kind: ProxySourceKind::ProxyV1,
    })
}

fn parse_port_strict(value: &str) -> Result<u16, ProxyParseError> {
    if value.is_empty() || value.len() > 5 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ProxyParseError::Invalid);
    }
    // Reject leading zeros? No: allow `0` and `080`; numeric range is the
    // authority. Empty and non-digit already rejected.
    value.parse::<u16>().map_err(|_| ProxyParseError::Invalid)
}

/// Parse a PROXY v2 header from the front of `buf`.
///
/// Returns the endpoints plus total consumed bytes (`16 + len`). Returns
/// [`ProxyParseError::Incomplete`] when fewer than the framed bytes are
/// available, [`ProxyParseError::TooLong`] when `len` exceeds
/// [`PROXY_V2_MAX_LEN`], and [`ProxyParseError::Invalid`] for signature,
/// version, command, family, or protocol violations.
///
/// `LOCAL`, `UNSPEC`, and `UNIX` preserve truthful absence (`None`
/// endpoints with `ProxyV2` provenance). TLVs beyond the address bytes are
/// ignored but remain bounded by the length ceiling.
pub fn parse_proxy_v2_header(buf: &[u8]) -> Result<(ProxyEndpoints, usize), ProxyParseError> {
    if buf.len() < PROXY_V2_HEADER_LEN {
        return Err(ProxyParseError::Incomplete);
    }
    if buf[..12] != PROXY_V2_SIGNATURE {
        return Err(ProxyParseError::Invalid);
    }
    let ver_cmd = buf[12];
    if ver_cmd >> 4 != 0x2 {
        return Err(ProxyParseError::Invalid);
    }
    let command = ver_cmd & 0x0F;
    if command != 0x00 && command != 0x01 {
        return Err(ProxyParseError::Invalid);
    }
    let fam_proto = buf[13];
    let family = fam_proto >> 4;
    let protocol = fam_proto & 0x0F;
    // Supported families: UNSPEC(0), INET(1), INET6(2), UNIX(3).
    if family > 0x03 {
        return Err(ProxyParseError::Invalid);
    }
    // Supported protocols: UNSPEC(0), STREAM(1), DGRAM(2). Others rejected.
    if protocol > 0x02 {
        return Err(ProxyParseError::Invalid);
    }
    let len = u16::from_be_bytes([buf[14], buf[15]]) as usize;
    if len > PROXY_V2_MAX_LEN {
        return Err(ProxyParseError::TooLong);
    }
    let total = PROXY_V2_HEADER_LEN + len;
    if buf.len() < total {
        return Err(ProxyParseError::Incomplete);
    }
    let body = &buf[PROXY_V2_HEADER_LEN..total];

    // LOCAL ignores all address information without inventing identity.
    if command == 0x00 {
        return Ok((
            ProxyEndpoints {
                source: None,
                destination: None,
                kind: ProxySourceKind::ProxyV2,
            },
            total,
        ));
    }

    // PROXY command with explicit family/protocol matrix. Only the
    // documented combinations are accepted; everything else fails closed.
    match (family, protocol) {
        (0x00, 0x00) => Ok((
            ProxyEndpoints {
                source: None,
                destination: None,
                kind: ProxySourceKind::ProxyV2,
            },
            total,
        )),
        (0x01, 0x01) => {
            if len < 12 {
                return Err(ProxyParseError::Invalid);
            }
            let src_ip = Ipv4Addr::new(body[0], body[1], body[2], body[3]);
            let dst_ip = Ipv4Addr::new(body[4], body[5], body[6], body[7]);
            let src_port = u16::from_be_bytes([body[8], body[9]]);
            let dst_port = u16::from_be_bytes([body[10], body[11]]);
            Ok((
                ProxyEndpoints {
                    source: Some(SocketAddr::new(IpAddr::V4(src_ip), src_port)),
                    destination: Some(SocketAddr::new(IpAddr::V4(dst_ip), dst_port)),
                    kind: ProxySourceKind::ProxyV2,
                },
                total,
            ))
        }
        (0x02, 0x01) => {
            if len < 36 {
                return Err(ProxyParseError::Invalid);
            }
            let mut src_bytes = [0u8; 16];
            let mut dst_bytes = [0u8; 16];
            src_bytes.copy_from_slice(&body[..16]);
            dst_bytes.copy_from_slice(&body[16..32]);
            let src_ip = Ipv6Addr::from(src_bytes);
            let dst_ip = Ipv6Addr::from(dst_bytes);
            let src_port = u16::from_be_bytes([body[32], body[33]]);
            let dst_port = u16::from_be_bytes([body[34], body[35]]);
            Ok((
                ProxyEndpoints {
                    source: Some(SocketAddr::new(IpAddr::V6(src_ip), src_port)),
                    destination: Some(SocketAddr::new(IpAddr::V6(dst_ip), dst_port)),
                    kind: ProxySourceKind::ProxyV2,
                },
                total,
            ))
        }
        (0x03, 0x01) => {
            // UNIX addresses have no SocketAddr mapping; truthful absence.
            if len < 216 {
                return Err(ProxyParseError::Invalid);
            }
            Ok((
                ProxyEndpoints {
                    source: None,
                    destination: None,
                    kind: ProxySourceKind::ProxyV2,
                },
                total,
            ))
        }
        _ => Err(ProxyParseError::Invalid),
    }
}

// ---------------------------------------------------------------------------
// Forwarded header parsing (Track D)
// ---------------------------------------------------------------------------

/// Why header-derived forwarding was not adopted (sanitized, no header bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardedRejection {
    /// Immediate peer is not trusted (includes Unix without explicit trust).
    UntrustedPeer,
    /// No forwarding policy is enabled.
    Disabled,
    /// Standardized and legacy headers disagree while both are enabled.
    Conflict,
    /// Total forwarded-header bytes exceed the configured ceiling.
    TooLarge,
    /// Chain elements exceed the configured ceiling.
    TooMany,
    /// A forwarded value is malformed.
    Invalid,
}

impl fmt::Display for ForwardedRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UntrustedPeer => write!(f, "untrusted peer"),
            Self::Disabled => write!(f, "forwarding disabled"),
            Self::Conflict => write!(f, "conflicting forwarded headers"),
            Self::TooLarge => write!(f, "forwarded headers exceed size limit"),
            Self::TooMany => write!(f, "forwarded chain exceeds element limit"),
            Self::Invalid => write!(f, "invalid forwarded headers"),
        }
    }
}

/// Trusted header-derived effective metadata (single-hop, rightmost wins).
///
/// `client` is `None` for `unknown`/obfuscated identifiers (truthful
/// absence); `scheme`/`authority` are `None` when absent or invalid.
/// `provenance` names the header family that produced the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedEffective {
    /// Effective client endpoint (port 0 when the header carries no port).
    pub client: Option<SocketAddr>,
    /// Effective external scheme asserted by the trusted proxy.
    pub scheme: Option<Scheme>,
    /// Effective external authority asserted by the trusted proxy.
    pub authority: Option<Authority>,
    /// Which header family produced this value.
    pub provenance: ProxySourceKind,
}

/// A parsed `Forwarded` element (`for=` / `proto=` / `host=` subset).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ForwardedElement {
    forward_for: ForwardedFor,
    proto: Option<Scheme>,
    host: Option<Authority>,
}

/// A parsed `for=` identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ForwardedFor {
    Unknown,
    Obfuscated,
    Ip(IpAddr, Option<u16>),
}

/// Derive trusted header metadata for one request.
///
/// `trusted_peer` must already reflect the immediate transport peer check
/// (Unix callers pass `config.trust_unix`). Untrusted peers, disabled
/// policy, conflicts, oversized chains, and malformed values all fail closed
/// to `Ok(None)` with a categorized rejection for observability; they never
/// produce trusted facts. The canonical `Host`/request-target is never
/// mutated here.
pub fn derive_forwarded_effective(
    headers: &HeaderBlock,
    config: &ForwardedConfig,
    trusted_peer: bool,
) -> Result<Option<ForwardedEffective>, ForwardedRejection> {
    let forwarded_values = headers.get_all("forwarded");
    let xff_values = headers.get_all("x-forwarded-for");
    let xfp_values = headers.get_all("x-forwarded-proto");
    let xfh_values = headers.get_all("x-forwarded-host");

    let has_standard = !forwarded_values.is_empty();
    let has_legacy = !xff_values.is_empty() || !xfp_values.is_empty() || !xfh_values.is_empty();

    // No forwarding headers at all: absent, not a rejection (avoids log spam
    // for ordinary direct requests). Trust and policy checks only matter
    // when there is something to adopt.
    let has_enabled_standard = has_standard && config.standard_enabled;
    let has_enabled_legacy = has_legacy && config.legacy_enabled;
    if !has_enabled_standard && !has_enabled_legacy {
        // If headers are present but their family is disabled, treat as
        // absent (untrusted by configuration, no observability noise).
        // If no headers at all, likewise absent.
        return Ok(None);
    }

    if !trusted_peer {
        return Err(ForwardedRejection::UntrustedPeer);
    }
    if config.is_disabled() {
        return Err(ForwardedRejection::Disabled);
    }

    // Conflict fails closed when both families are enabled and present.
    if has_standard && has_legacy && config.standard_enabled && config.legacy_enabled {
        return Err(ForwardedRejection::Conflict);
    }

    // Enforce the byte ceiling before parsing (sum of relevant values).
    let mut total_bytes = 0usize;
    if config.standard_enabled {
        for value in &forwarded_values {
            total_bytes = total_bytes.saturating_add(value.as_bytes().len());
        }
    }
    if config.legacy_enabled {
        for value in xff_values
            .iter()
            .chain(xfp_values.iter())
            .chain(xfh_values.iter())
        {
            total_bytes = total_bytes.saturating_add(value.as_bytes().len());
        }
    }
    if total_bytes > config.max_bytes {
        return Err(ForwardedRejection::TooLarge);
    }

    if has_standard && config.standard_enabled {
        let joined = join_header_values(&forwarded_values);
        return match parse_forwarded_chain(&joined, config) {
            Ok(effective) => Ok(Some(effective)),
            Err(ForwardedRejection::TooMany) => Err(ForwardedRejection::TooMany),
            Err(_) => Err(ForwardedRejection::Invalid),
        };
    }

    if has_legacy && config.legacy_enabled {
        let effective = parse_legacy_forwarded(&xff_values, &xfp_values, &xfh_values, config)?;
        // No legacy values actually enabled? Treat as absent.
        return Ok(effective);
    }

    Ok(None)
}

fn join_header_values(values: &[&crate::primitives::header_block::HeaderValue]) -> String {
    let mut out = String::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        // Header values are octet-preserving; lossy display is only for
        // parsing (invalid UTF-8 fails closed below as Invalid).
        out.push_str(&String::from_utf8_lossy(value.as_bytes()));
    }
    out
}

fn parse_forwarded_chain(
    joined: &str,
    config: &ForwardedConfig,
) -> Result<ForwardedEffective, ForwardedRejection> {
    if joined.len() > config.max_bytes {
        return Err(ForwardedRejection::TooLarge);
    }
    // Split top-level on commas (no quoted commas in practice for the
    // subset we honor; quoted `for="..."` values never contain a comma
    // outside brackets in valid input, and anything ambiguous fails closed).
    let raw_elements: Vec<&str> = joined
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if raw_elements.is_empty() {
        return Err(ForwardedRejection::Invalid);
    }
    if raw_elements.len() > config.max_elements {
        return Err(ForwardedRejection::TooMany);
    }
    let mut elements = Vec::with_capacity(raw_elements.len());
    for raw in raw_elements {
        elements.push(parse_forwarded_element(raw)?);
    }
    // Single-hop rightmost wins: the last element is closest to the trusted proxy.
    let last = elements
        .last()
        .cloned()
        .ok_or(ForwardedRejection::Invalid)?;
    // Rightmost defined proto/host wins (a replacing proxy emits one
    // element; an appending proxy adds proto/host on its own element).
    let mut scheme = last.proto;
    let mut authority = last.host.clone();
    if scheme.is_none() || authority.is_none() {
        for element in elements.iter().rev() {
            if scheme.is_none() {
                scheme = element.proto;
            }
            if authority.is_none() {
                authority = element.host.clone();
            }
            if scheme.is_some() && authority.is_some() {
                break;
            }
        }
    }
    let client = match last.forward_for {
        ForwardedFor::Unknown | ForwardedFor::Obfuscated => None,
        ForwardedFor::Ip(ip, port) => Some(SocketAddr::new(ip, port.unwrap_or(0))),
    };
    // If every derived value is absent, there is still provenance (the
    // proxy asserted `unknown`), but callers treat it as no effective
    // identity. Return it so observability can record acceptance.
    Ok(ForwardedEffective {
        client,
        scheme,
        authority,
        provenance: ProxySourceKind::Forwarded,
    })
}

fn parse_forwarded_element(raw: &str) -> Result<ForwardedElement, ForwardedRejection> {
    let mut forward_for: Option<ForwardedFor> = None;
    let mut proto: Option<Scheme> = None;
    let mut host: Option<Authority> = None;
    // Semicolon-separated pairs; empty segments are malformed.
    for pair in raw.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            return Err(ForwardedRejection::Invalid);
        }
        let (key, value) = pair.split_once('=').ok_or(ForwardedRejection::Invalid)?;
        let key = key.trim().to_ascii_lowercase();
        let value = unquote_forwarded_value(value.trim())?;
        match key.as_str() {
            "for" => {
                forward_for = Some(parse_forwarded_for_identifier(&value)?);
            }
            "proto" => {
                // Only http/https are meaningful origins; anything else is
                // ignored for scheme (client/host may still apply).
                match value.to_ascii_lowercase().as_str() {
                    "http" => proto = Some(Scheme::Http),
                    "https" => proto = Some(Scheme::Https),
                    _ => {}
                }
            }
            "host" => {
                if let Ok(authority) = Authority::parse(&value) {
                    host = Some(authority);
                }
                // Invalid host is ignored for host (client/scheme may apply).
            }
            "by" => {
                // Ignored by policy (topology hiding).
            }
            _ => {
                // Unknown pairs are ignored (forward compatibility).
            }
        }
    }
    let forward_for = forward_for.ok_or(ForwardedRejection::Invalid)?;
    Ok(ForwardedElement {
        forward_for,
        proto,
        host,
    })
}

fn unquote_forwarded_value(value: &str) -> Result<String, ForwardedRejection> {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        // Minimal quoted-string unescape (\" and \\); anything else with a
        // bare backslash fails closed.
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    _ => return Err(ForwardedRejection::Invalid),
                }
            } else if ch == '"' {
                return Err(ForwardedRejection::Invalid);
            } else {
                out.push(ch);
            }
        }
        Ok(out)
    } else {
        if value.contains('"') || value.contains('\\') {
            return Err(ForwardedRejection::Invalid);
        }
        Ok(value.to_owned())
    }
}

fn parse_forwarded_for_identifier(value: &str) -> Result<ForwardedFor, ForwardedRejection> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ForwardedRejection::Invalid);
    }
    if value.eq_ignore_ascii_case("unknown") {
        return Ok(ForwardedFor::Unknown);
    }
    if value.starts_with('_') {
        // Obfuscated identifiers (`_hidden`, `_SEVKISEK`, ...).
        if value.len() == 1
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        {
            return Err(ForwardedRejection::Invalid);
        }
        return Ok(ForwardedFor::Obfuscated);
    }
    // Bracketed IPv6 with optional port: `[::1]` or `[::1]:4711`.
    if value.starts_with('[') {
        let close = value.find(']').ok_or(ForwardedRejection::Invalid)?;
        let host_part = &value[1..close];
        let ip: Ipv6Addr = host_part.parse().map_err(|_| ForwardedRejection::Invalid)?;
        let rest = &value[close + 1..];
        if rest.is_empty() {
            return Ok(ForwardedFor::Ip(IpAddr::V6(ip), None));
        }
        let port_str = rest.strip_prefix(':').ok_or(ForwardedRejection::Invalid)?;
        let port = parse_port_strict(port_str).map_err(|_| ForwardedRejection::Invalid)?;
        return Ok(ForwardedFor::Ip(IpAddr::V6(ip), Some(port)));
    }
    // Bare IP with optional port. IPv6 without brackets contains multiple
    // colons and never carries a port here.
    let colon_count = value.bytes().filter(|b| *b == b':').count();
    if colon_count > 1 {
        let ip: Ipv6Addr = value.parse().map_err(|_| ForwardedRejection::Invalid)?;
        return Ok(ForwardedFor::Ip(IpAddr::V6(ip), None));
    }
    if colon_count == 1 {
        // Could be IPv4:port. Split once from the right.
        if let Some((host_part, port_part)) = value.rsplit_once(':') {
            if let Ok(ipv4) = host_part.parse::<Ipv4Addr>() {
                let port = parse_port_strict(port_part).map_err(|_| ForwardedRejection::Invalid)?;
                return Ok(ForwardedFor::Ip(IpAddr::V4(ipv4), Some(port)));
            }
            // Single colon but not IPv4:port is malformed (bare IPv6 must
            // use brackets when a port is present).
            return Err(ForwardedRejection::Invalid);
        }
        return Err(ForwardedRejection::Invalid);
    }
    // No colon: bare IPv4.
    if let Ok(ipv4) = value.parse::<Ipv4Addr>() {
        return Ok(ForwardedFor::Ip(IpAddr::V4(ipv4), None));
    }
    // Anything else (hostnames, obfuscated without underscore) is rejected
    // in the first implementation; only IP literals, `unknown`, and
    // underscore-obfuscated identifiers are honored.
    Err(ForwardedRejection::Invalid)
}

fn parse_legacy_forwarded(
    xff_values: &[&crate::primitives::header_block::HeaderValue],
    xfp_values: &[&crate::primitives::header_block::HeaderValue],
    xfh_values: &[&crate::primitives::header_block::HeaderValue],
    config: &ForwardedConfig,
) -> Result<Option<ForwardedEffective>, ForwardedRejection> {
    // Combine duplicate lines with commas (standard list-header semantics).
    let xff_joined = join_header_values(xff_values);
    let xfp_joined = join_header_values(xfp_values);
    let xfh_joined = join_header_values(xfh_values);

    let mut client: Option<SocketAddr> = None;
    let mut scheme: Option<Scheme> = None;
    let mut authority: Option<Authority> = None;

    if !xff_joined.trim().is_empty() {
        let parts: Vec<&str> = xff_joined
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(ForwardedRejection::Invalid);
        }
        if parts.len() > config.max_elements {
            return Err(ForwardedRejection::TooMany);
        }
        let mut addrs = Vec::with_capacity(parts.len());
        for part in parts {
            addrs.push(parse_x_forwarded_for_entry(part)?);
        }
        // Rightmost (closest to the trusted proxy) wins.
        client = addrs.last().copied();
    }

    if !xfp_joined.trim().is_empty() {
        let parts: Vec<&str> = xfp_joined
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(ForwardedRejection::Invalid);
        }
        if parts.len() > config.max_elements {
            return Err(ForwardedRejection::TooMany);
        }
        let mut last_scheme: Option<Scheme> = None;
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "http" => last_scheme = Some(Scheme::Http),
                "https" => last_scheme = Some(Scheme::Https),
                _ => return Err(ForwardedRejection::Invalid),
            }
        }
        scheme = last_scheme;
    }

    if !xfh_joined.trim().is_empty() {
        let parts: Vec<&str> = xfh_joined
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(ForwardedRejection::Invalid);
        }
        if parts.len() > config.max_elements {
            return Err(ForwardedRejection::TooMany);
        }
        let mut last_authority: Option<Authority> = None;
        for part in parts {
            let unquoted = part
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(part);
            last_authority =
                Some(Authority::parse(unquoted).map_err(|_| ForwardedRejection::Invalid)?);
        }
        authority = last_authority;
    }

    if client.is_none() && scheme.is_none() && authority.is_none() {
        return Ok(None);
    }
    Ok(Some(ForwardedEffective {
        client,
        scheme,
        authority,
        provenance: ProxySourceKind::LegacyForwarded,
    }))
}

fn parse_x_forwarded_for_entry(value: &str) -> Result<SocketAddr, ForwardedRejection> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ForwardedRejection::Invalid);
    }
    let unquoted = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value);
    // Bracketed IPv6 with optional port.
    if unquoted.starts_with('[') {
        let close = unquoted.find(']').ok_or(ForwardedRejection::Invalid)?;
        let host_part = &unquoted[1..close];
        let ip: Ipv6Addr = host_part.parse().map_err(|_| ForwardedRejection::Invalid)?;
        let rest = &unquoted[close + 1..];
        if rest.is_empty() {
            return Ok(SocketAddr::new(IpAddr::V6(ip), 0));
        }
        let port_str = rest.strip_prefix(':').ok_or(ForwardedRejection::Invalid)?;
        let port = parse_port_strict(port_str).map_err(|_| ForwardedRejection::Invalid)?;
        return Ok(SocketAddr::new(IpAddr::V6(ip), port));
    }
    // Bare IPv4 or IPv6 (no port when ambiguous).
    if let Ok(ipv4) = unquoted.parse::<Ipv4Addr>() {
        return Ok(SocketAddr::new(IpAddr::V4(ipv4), 0));
    }
    if let Ok(ipv6) = unquoted.parse::<Ipv6Addr>() {
        return Ok(SocketAddr::new(IpAddr::V6(ipv6), 0));
    }
    // IPv4 with port (`1.2.3.4:5678`).
    if let Some((host_part, port_part)) = unquoted.rsplit_once(':') {
        if host_part.contains(':') {
            return Err(ForwardedRejection::Invalid);
        }
        if let Ok(ipv4) = host_part.parse::<Ipv4Addr>() {
            let port = parse_port_strict(port_part).map_err(|_| ForwardedRejection::Invalid)?;
            return Ok(SocketAddr::new(IpAddr::V4(ipv4), port));
        }
    }
    Err(ForwardedRejection::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_prefix_exact_and_cidr_matching() {
        let exact = IpPrefix::parse("192.168.0.10").unwrap();
        assert!(exact.matches(&"192.168.0.10".parse().unwrap()));
        assert!(!exact.matches(&"192.168.0.11".parse().unwrap()));

        let cidr = IpPrefix::parse("192.168.0.0/24").unwrap();
        assert!(cidr.matches(&"192.168.0.1".parse().unwrap()));
        assert!(!cidr.matches(&"192.168.1.1".parse().unwrap()));

        let v6 = IpPrefix::parse("2001:db8::/32").unwrap();
        assert!(v6.matches(&"2001:db8::1".parse().unwrap()));
        assert!(!v6.matches(&"2001:db9::1".parse().unwrap()));

        // Families never cross-match.
        assert!(!cidr.matches(&"::1".parse().unwrap()));
    }

    #[test]
    fn ip_prefix_rejects_dns_and_bad_prefix() {
        assert!(IpPrefix::parse("proxy.internal").is_err());
        assert!(IpPrefix::parse("").is_err());
        assert!(IpPrefix::parse("192.168.0.0/33").is_err());
        assert!(IpPrefix::parse("::1/129").is_err());
    }

    #[test]
    fn proxy_v1_tcp4_round_trip() {
        let endpoints =
            parse_proxy_v1_line(b"PROXY TCP4 192.168.0.1 192.168.0.11 56324 443\r\n").unwrap();
        assert_eq!(endpoints.kind, ProxySourceKind::ProxyV1);
        assert_eq!(endpoints.source.unwrap().to_string(), "192.168.0.1:56324");
        assert_eq!(
            endpoints.destination.unwrap().to_string(),
            "192.168.0.11:443"
        );
    }

    #[test]
    fn proxy_v1_unknown_preserves_absence() {
        let endpoints = parse_proxy_v1_line(b"PROXY UNKNOWN\r\n").unwrap();
        assert!(endpoints.source.is_none());
        assert!(endpoints.destination.is_none());
        assert_eq!(endpoints.kind, ProxySourceKind::ProxyV1);
    }

    #[test]
    fn proxy_v1_rejects_bad_family_and_spacing() {
        assert!(parse_proxy_v1_line(b"PROXY TCP4 1.1.1.1 2.2.2.2 1 2 3\r\n").is_err());
        assert!(parse_proxy_v1_line(b"PROXY  TCP4 1.1.1.1 2.2.2.2 1 80\r\n").is_err());
        assert!(parse_proxy_v1_line(b"PROXY UNIX a b\r\n").is_err());
        assert!(parse_proxy_v1_line(b"PROXY TCP4 1.1.1.1 2.2.2.2 99999 80\r\n").is_err());
    }

    #[test]
    fn proxy_v1_rejects_oversized() {
        let mut long = b"PROXY TCP4 1.1.1.1 2.2.2.2 1 80".to_vec();
        long.resize(PROXY_V1_MAX_BYTES + 1, b'X');
        assert_eq!(
            parse_proxy_v1_line(&long).unwrap_err(),
            ProxyParseError::TooLong
        );
    }

    #[test]
    fn proxy_v2_ipv4_round_trip() {
        let mut header = Vec::new();
        header.extend_from_slice(&PROXY_V2_SIGNATURE);
        header.push(0x21); // v2 + PROXY
        header.push(0x11); // INET + STREAM
        header.extend_from_slice(&12u16.to_be_bytes());
        header.extend_from_slice(&[192, 168, 0, 1, 192, 168, 0, 11]);
        header.extend_from_slice(&56324u16.to_be_bytes());
        header.extend_from_slice(&443u16.to_be_bytes());
        let (endpoints, consumed) = parse_proxy_v2_header(&header).unwrap();
        assert_eq!(consumed, header.len());
        assert_eq!(endpoints.kind, ProxySourceKind::ProxyV2);
        assert_eq!(endpoints.source.unwrap().to_string(), "192.168.0.1:56324");
    }

    #[test]
    fn proxy_v2_local_and_unspec_preserve_absence() {
        let mut local = Vec::new();
        local.extend_from_slice(&PROXY_V2_SIGNATURE);
        local.push(0x20); // v2 + LOCAL
        local.push(0x11);
        local.extend_from_slice(&12u16.to_be_bytes());
        local.extend_from_slice(&[0u8; 12]);
        let (endpoints, _) = parse_proxy_v2_header(&local).unwrap();
        assert!(endpoints.source.is_none());

        let mut unspec = Vec::new();
        unspec.extend_from_slice(&PROXY_V2_SIGNATURE);
        unspec.push(0x21);
        unspec.push(0x00);
        unspec.extend_from_slice(&0u16.to_be_bytes());
        let (endpoints, _) = parse_proxy_v2_header(&unspec).unwrap();
        assert!(endpoints.source.is_none());
    }

    #[test]
    fn proxy_v2_rejects_bad_signature_and_oversized_len() {
        let bad = vec![0u8; 16];
        assert!(parse_proxy_v2_header(&bad).is_err());
        let mut oversized = Vec::new();
        oversized.extend_from_slice(&PROXY_V2_SIGNATURE);
        oversized.push(0x21);
        oversized.push(0x11);
        oversized.extend_from_slice(&(PROXY_V2_MAX_LEN as u16 + 1).to_be_bytes());
        assert_eq!(
            parse_proxy_v2_header(&oversized).unwrap_err(),
            ProxyParseError::TooLong
        );
    }

    #[test]
    fn proxy_v2_needs_more_bytes_when_truncated() {
        let mut header = Vec::new();
        header.extend_from_slice(&PROXY_V2_SIGNATURE);
        header.push(0x21);
        header.push(0x11);
        header.extend_from_slice(&12u16.to_be_bytes());
        header.extend_from_slice(&[192, 168, 0, 1]);
        assert_eq!(
            parse_proxy_v2_header(&header).unwrap_err(),
            ProxyParseError::Incomplete
        );
    }
}
