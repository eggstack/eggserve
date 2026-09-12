//! Provenance values for trusted proxy metadata.

use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpPrefix {
    pub address: IpAddr,
    pub prefix_len: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyProvenance {
    Direct,
    ProxyProtocol,
    Forwarded,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxyConfig {
    pub peers: Vec<IpPrefix>,
    pub trust_unix: bool,
    pub proxy_protocol: bool,
    pub forwarded: bool,
}
