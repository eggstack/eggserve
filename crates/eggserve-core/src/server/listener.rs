//! Listener ownership and process-manager integration (Plan 201).
//!
//! This module owns the listener-adoption vocabulary shared by
//! [`crate::server::ServerBuilder`] and [`crate::server::ServerHandle`].
//! It does not implement a second accept loop: every adopted listener feeds
//! the single canonical accept/admission/TLS/lifecycle pipeline in
//! [`crate::server`].
//!
//! # Ownership model
//!
//! All listener constructors take ownership:
//!
//! - [`crate::server::ServerBuilder::from_listener`] takes a Tokio TCP
//!   listener; [`crate::server::ServerBuilder::from_std_listener`] takes a
//!   standard-library TCP listener and normalizes nonblocking mode.
//! - Unix variants (Unix only) take an already-bound Unix listener. EggServe
//!   never unlinks filesystem socket paths: the caller owns creation and
//!   removal. Abstract-namespace sockets carry no filesystem path and need no
//!   cleanup.
//! - systemd adoption ([`adopt_systemd_listener`]) validates the inherited
//!   descriptor and takes ownership on success. Failed validation never
//!   closes the descriptor.
//! - H3 prebound UDP ([`crate::server::ServerBuilder::http3_socket`]) takes a
//!   bound standard-library UDP socket; Quinn wraps it at startup without
//!   exposing Quinn types in the public contract.
//!
//! Socket options set by the caller are preserved except where correctness
//! requires normalization (nonblocking mode is always enabled for async
//! use). Callers choosing `SO_REUSEPORT`, network namespaces, or other
//! socket policy do so before handing the socket over.
//!
//! # Endpoint identities
//!
//! [`BoundEndpoint`] describes every successfully adopted listener with a
//! stable string ID (`tcp-0`, `unix-0`, ...) rather than a positional index.
//! [`crate::server::ServerHandle::endpoints`] exposes the full set;
//! [`crate::server::ServerHandle::local_addr`] preserves the common
//! one-TCP-listener path.

// Systemd descriptor adoption is the only unsafe boundary here. The raw fd is
// borrowed for validation, then owned exactly once after the socket checks;
// all other listener paths remain safe Rust/Tokio APIs.
#![allow(unsafe_code)]

use std::net::SocketAddr;
#[cfg(unix)]
use std::path::PathBuf;

/// A successfully bound/adopted listener endpoint.
///
/// Stable string IDs (`tcp-0`, `unix-0`) identify listeners in logs and
/// metrics; never rely on vector position.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundEndpoint {
    /// A TCP listener with its observed local socket address.
    Tcp {
        /// Stable listener ID (for example `tcp-0`).
        id: String,
        /// Observed local address (port zero already resolved).
        addr: SocketAddr,
    },
    /// A Unix-domain listener (Unix only).
    ///
    /// `path` is `Some` for filesystem-bound sockets and `None` for
    /// abstract-namespace or unnamed sockets.
    #[cfg(unix)]
    Unix {
        /// Stable listener ID (for example `unix-0`).
        id: String,
        /// Filesystem socket path, when the address has one.
        path: Option<PathBuf>,
    },
}

impl BoundEndpoint {
    /// Stable listener ID.
    pub fn id(&self) -> &str {
        match self {
            Self::Tcp { id, .. } => id,
            #[cfg(unix)]
            Self::Unix { id, .. } => id,
        }
    }

    /// Returns `true` for TCP endpoints.
    pub fn is_tcp(&self) -> bool {
        matches!(self, Self::Tcp { .. })
    }

    /// TCP local address, when this endpoint is TCP.
    pub fn tcp_addr(&self) -> Option<SocketAddr> {
        match self {
            Self::Tcp { addr, .. } => Some(*addr),
            #[cfg(unix)]
            Self::Unix { .. } => None,
        }
    }

    /// Unix socket filesystem path, when bound to one (Unix only).
    #[cfg(unix)]
    pub fn unix_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Tcp { .. } => None,
            Self::Unix { path, .. } => path.as_ref(),
        }
    }
}

impl std::fmt::Display for BoundEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp { id, addr } => write!(f, "{id} tcp/{addr}"),
            #[cfg(unix)]
            Self::Unix { id, path } => match path {
                Some(p) => write!(f, "{id} unix/{}", p.display()),
                None => write!(f, "{id} unix/abstract-or-unnamed"),
            },
        }
    }
}

/// Normalize a caller-bound standard-library TCP listener for async use.
///
/// Enables nonblocking mode (required for Tokio) and converts via
/// `Tokio TcpListener::from_std`. All other socket options are preserved.
pub(crate) fn normalize_std_tcp_listener(
    listener: std::net::TcpListener,
) -> Result<tokio::net::TcpListener, std::io::Error> {
    listener.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(listener)
}

/// Normalize a caller-bound standard-library Unix listener (Unix only).
///
/// Enables nonblocking mode; filesystem path ownership stays with the
/// caller (EggServe never unlinks).
#[cfg(unix)]
pub(crate) fn normalize_std_unix_listener(
    listener: std::os::unix::net::UnixListener,
) -> Result<tokio::net::UnixListener, std::io::Error> {
    listener.set_nonblocking(true)?;
    tokio::net::UnixListener::from_std(listener)
}

/// Filesystem path of a bound Unix listener, if it has one.
///
/// Returns `None` for abstract-namespace or unnamed sockets.
#[cfg(unix)]
pub(crate) fn unix_listener_path(listener: &tokio::net::UnixListener) -> Option<PathBuf> {
    listener
        .local_addr()
        .ok()
        .and_then(|addr| addr.as_pathname().map(PathBuf::from))
}

// ---------------------------------------------------------------------------
// systemd socket activation (Unix only)
// ---------------------------------------------------------------------------

/// An adopted systemd-style inherited listener (Unix only).
///
/// Produced by [`adopt_systemd_listener`] / [`adopt_systemd_listener_by_name`]
/// after validating descriptor type, domain, and listening state. Ownership
/// transfers to the caller on success; validation failure never closes the
/// descriptor.
#[cfg(unix)]
#[derive(Debug)]
pub(crate) enum SystemdListener {
    Tcp(tokio::net::TcpListener),
    Unix(tokio::net::UnixListener),
}

/// Number of descriptors passed via `LISTEN_FDS` for this process.
///
/// Returns `0` when this process was not socket-activated (`LISTEN_PID`
/// absent or not equal to the current PID, or `LISTEN_FDS` absent/zero).
/// Malformed values produce a [`crate::server::errors::ServerError::Config`]
/// error rather than silent truncation.
#[cfg(unix)]
pub fn systemd_activation_count() -> Result<usize, crate::server::errors::ServerError> {
    use crate::server::errors::ServerError;

    let pid_var = std::env::var("LISTEN_PID").unwrap_or_default();
    if pid_var.is_empty() {
        return Ok(0);
    }
    let pid: u32 = pid_var
        .parse()
        .map_err(|_| ServerError::Config("invalid LISTEN_PID".into()))?;
    if pid != std::process::id() {
        return Ok(0);
    }
    let fds_var = std::env::var("LISTEN_FDS").unwrap_or_default();
    if fds_var.is_empty() {
        return Ok(0);
    }
    let count: usize = fds_var
        .parse()
        .map_err(|_| ServerError::Config("invalid LISTEN_FDS".into()))?;
    Ok(count)
}

/// Remove socket-activation inheritance state from this process's environment.
///
/// Call after successful adoption when this process will spawn children that
/// must not inherit activation semantics. EggServe never clears these
/// variables automatically: global environment mutation stays an explicit
/// caller decision.
#[cfg(unix)]
pub fn clear_systemd_activation_env() {
    std::env::remove_var("LISTEN_PID");
    std::env::remove_var("LISTEN_FDS");
    std::env::remove_var("LISTEN_FDNAMES");
}

/// Adopt the `index`-th systemd/socket-activation descriptor.
///
/// Descriptors start at fd 3 (`SD_LISTEN_FDS_START`); `index` is an explicit
/// offset from there. The descriptor is validated before ownership is taken:
///
/// - fd must be a `SOCK_STREAM` socket (datagram descriptors are rejected
///   from the TCP/Unix-stream listener path);
/// - the socket must be in listening state (`SO_ACCEPTCONN`);
/// - `AF_INET`/`AF_INET6` adopts as TCP, `AF_UNIX` adopts as a Unix stream
///   listener; any other family is rejected.
///
/// A connected (non-listening) socket passed as a listener is rejected
/// rather than served.
#[cfg(unix)]
pub(crate) fn adopt_systemd_listener(
    index: usize,
) -> Result<SystemdListener, crate::server::errors::ServerError> {
    use crate::server::errors::ServerError;
    use std::os::fd::{BorrowedFd, OwnedFd};

    let count = systemd_activation_count()?;
    if count == 0 {
        return Err(ServerError::Config(
            "no systemd socket-activation descriptors for this process".into(),
        ));
    }
    if index >= count {
        return Err(ServerError::Config(format!(
            "systemd descriptor index {index} out of range (count={count})"
        )));
    }
    // SD_LISTEN_FDS_START = 3.
    let raw_fd = 3 + index as std::os::fd::RawFd;

    // Validate without taking ownership first so failure never closes a
    // descriptor the caller (or manager) still owns.
    // SAFETY: `raw_fd` is only borrowed for `getsockopt`/`getsockname`
    // validation here; ownership is taken exactly once below on success.
    let borrowed = unsafe { BorrowedFd::borrow_raw(raw_fd) };
    validate_stream_listener_fd(borrowed, raw_fd)?;
    let family = rustix::net::getsockname(borrowed)
        .map(|addr| addr.address_family())
        .map_err(|e| ServerError::Config(format!("systemd fd {raw_fd} getsockname failed: {e}")))?;

    // Validation passed: take ownership exactly once.
    // SAFETY: the fd came from the manager (fd 3+N), was just validated as
    // an open listening stream socket, and is not owned elsewhere in this
    // process. On later conversion failure the OwnedFd drops (closes),
    // which is correct: a failed adoption owns nothing the caller can reuse.
    let owned: OwnedFd = unsafe { std::os::fd::FromRawFd::from_raw_fd(raw_fd) };

    if family == rustix::net::AddressFamily::UNIX {
        let std_listener: std::os::unix::net::UnixListener = owned.into();
        let tokio_listener =
            normalize_std_unix_listener(std_listener).map_err(ServerError::Bind)?;
        Ok(SystemdListener::Unix(tokio_listener))
    } else if family == rustix::net::AddressFamily::INET
        || family == rustix::net::AddressFamily::INET6
    {
        let std_listener: std::net::TcpListener = owned.into();
        let tokio_listener = normalize_std_tcp_listener(std_listener).map_err(ServerError::Bind)?;
        Ok(SystemdListener::Tcp(tokio_listener))
    } else {
        // Ownership was taken but the family is unsupported; drop (close)
        // rather than leak, and report explicitly.
        drop(owned);
        Err(ServerError::Config(format!(
            "systemd fd {raw_fd} has unsupported address family; only AF_INET/AF_INET6/AF_UNIX stream listeners are adopted"
        )))
    }
}

/// Validate that a borrowed descriptor is a listening `SOCK_STREAM` socket.
///
/// Shared by systemd adoption and descriptor-level tests: rejects datagram
/// descriptors and connected (non-listening) sockets before any ownership is
/// taken, so validation failure never closes the descriptor. `label` names
/// the fd in error messages (for example `systemd fd 3`).
#[cfg(unix)]
pub(crate) fn validate_stream_listener_fd(
    fd: std::os::fd::BorrowedFd<'_>,
    label: impl std::fmt::Display,
) -> Result<(), crate::server::errors::ServerError> {
    use crate::server::errors::ServerError;

    let sock_type = rustix::net::sockopt::socket_type(fd)
        .map_err(|e| ServerError::Config(format!("{label} is not a socket: {e}")))?;
    if sock_type != rustix::net::SocketType::STREAM {
        return Err(ServerError::Config(format!(
            "{label} is not SOCK_STREAM; datagram descriptors are rejected from the stream listener path"
        )));
    }
    let listening = rustix::net::sockopt::socket_acceptconn(fd)
        .map_err(|e| ServerError::Config(format!("{label} accept-conn probe failed: {e}")))?;
    if !listening {
        return Err(ServerError::Config(format!(
            "{label} is not in listening state; refusing to adopt a connected socket as a listener"
        )));
    }
    Ok(())
}

/// Adopt any validated listening stream fd (test helper, Unix only).
///
/// Validates with [`validate_stream_listener_fd`], then takes ownership via
/// `dup` (the caller's fd stays open so failed startup never closes a
/// descriptor the caller still owns) and converts by family. Used by
/// descriptor-level tests; systemd adoption takes ownership directly.
#[cfg(all(unix, test))]
pub(crate) fn adopt_validated_fd(
    fd: std::os::fd::BorrowedFd<'_>,
) -> Result<SystemdListener, crate::server::errors::ServerError> {
    use crate::server::errors::ServerError;

    validate_stream_listener_fd(fd, "descriptor")?;
    let family = rustix::net::getsockname(fd)
        .map(|addr| addr.address_family())
        .map_err(|e| ServerError::Config(format!("descriptor getsockname failed: {e}")))?;
    // Duplicate so the caller's fd remains owned by the caller on both
    // success and failure paths.
    let owned: std::os::fd::OwnedFd = rustix::io::dup(fd)
        .map_err(|e| ServerError::Config(format!("descriptor dup failed: {e}")))?;
    if family == rustix::net::AddressFamily::UNIX {
        let std_listener: std::os::unix::net::UnixListener = owned.into();
        let tokio_listener =
            normalize_std_unix_listener(std_listener).map_err(ServerError::Bind)?;
        Ok(SystemdListener::Unix(tokio_listener))
    } else if family == rustix::net::AddressFamily::INET
        || family == rustix::net::AddressFamily::INET6
    {
        let std_listener: std::net::TcpListener = owned.into();
        let tokio_listener = normalize_std_tcp_listener(std_listener).map_err(ServerError::Bind)?;
        Ok(SystemdListener::Tcp(tokio_listener))
    } else {
        Err(ServerError::Config(
            "descriptor has unsupported address family; only AF_INET/AF_INET6/AF_UNIX stream listeners are adopted".into(),
        ))
    }
}

/// Adopt a systemd descriptor by `LISTEN_FDNAMES` entry.
///
/// `LISTEN_FDNAMES` is a colon-separated list parallel to the fd array.
/// The name must match exactly; when the variable is absent there is no
/// implicit mapping and adoption fails rather than guessing fd 3.
#[cfg(unix)]
pub(crate) fn adopt_systemd_listener_by_name(
    name: &str,
) -> Result<SystemdListener, crate::server::errors::ServerError> {
    use crate::server::errors::ServerError;

    let names = std::env::var("LISTEN_FDNAMES").map_err(|_| {
        ServerError::Config("LISTEN_FDNAMES is not set; cannot map systemd listener name".into())
    })?;
    let index = names
        .split(':')
        .position(|entry| entry == name)
        .ok_or_else(|| {
            ServerError::Config(format!(
                "systemd listener name {name:?} not found in LISTEN_FDNAMES"
            ))
        })?;
    adopt_systemd_listener(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_endpoint_display_tcp() {
        let ep = BoundEndpoint::Tcp {
            id: "tcp-0".into(),
            addr: "127.0.0.1:8000".parse().unwrap(),
        };
        assert_eq!(ep.id(), "tcp-0");
        assert!(ep.is_tcp());
        assert!(ep.tcp_addr().is_some());
        assert!(format!("{ep}").contains("tcp-0"));
    }

    #[cfg(unix)]
    #[test]
    fn bound_endpoint_display_unix() {
        let ep = BoundEndpoint::Unix {
            id: "unix-0".into(),
            path: Some(PathBuf::from("/tmp/eggserve.sock")),
        };
        assert_eq!(ep.id(), "unix-0");
        assert!(!ep.is_tcp());
        assert!(ep.tcp_addr().is_none());
        assert!(format!("{ep}").contains("unix-0"));
    }

    #[cfg(unix)]
    #[test]
    fn systemd_count_zero_without_env() {
        // Save/restore to avoid cross-test interference.
        let pid = std::env::var("LISTEN_PID").ok();
        let fds = std::env::var("LISTEN_FDS").ok();
        std::env::remove_var("LISTEN_PID");
        std::env::remove_var("LISTEN_FDS");
        let count = systemd_activation_count().unwrap();
        assert_eq!(count, 0);
        if let Some(v) = pid {
            std::env::set_var("LISTEN_PID", v);
        }
        if let Some(v) = fds {
            std::env::set_var("LISTEN_FDS", v);
        }
    }

    #[cfg(unix)]
    #[test]
    fn systemd_adopt_rejects_without_activation() {
        let pid = std::env::var("LISTEN_PID").ok();
        let fds = std::env::var("LISTEN_FDS").ok();
        std::env::remove_var("LISTEN_PID");
        std::env::remove_var("LISTEN_FDS");
        let err = adopt_systemd_listener(0).unwrap_err();
        assert!(err.to_string().contains("no systemd"));
        if let Some(v) = pid {
            std::env::set_var("LISTEN_PID", v);
        }
        if let Some(v) = fds {
            std::env::set_var("LISTEN_FDS", v);
        }
    }

    #[cfg(unix)]
    #[test]
    fn systemd_adopt_rejects_out_of_range_index() {
        let pid = std::env::var("LISTEN_PID").ok();
        let fds = std::env::var("LISTEN_FDS").ok();
        std::env::set_var("LISTEN_PID", std::process::id().to_string());
        std::env::set_var("LISTEN_FDS", "1");
        let err = adopt_systemd_listener(7).unwrap_err();
        assert!(err.to_string().contains("out of range"));
        match (pid, fds) {
            (Some(p), Some(f)) => {
                std::env::set_var("LISTEN_PID", p);
                std::env::set_var("LISTEN_FDS", f);
            }
            _ => {
                std::env::remove_var("LISTEN_PID");
                std::env::remove_var("LISTEN_FDS");
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn validated_fd_adopts_tcp_listener_without_closing_original() {
        use std::os::fd::AsFd;

        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let adopted = adopt_validated_fd(std_listener.as_fd()).unwrap();
        assert!(matches!(adopted, SystemdListener::Tcp(_)));
        // Original fd stays owned by the caller (dup, not take).
        assert!(std_listener.local_addr().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn validated_fd_rejects_connected_socket_as_listener() {
        use std::os::fd::AsFd;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let connected = std::net::TcpStream::connect(addr).unwrap();
        let err = adopt_validated_fd(connected.as_fd()).unwrap_err();
        assert!(err.to_string().contains("not in listening state"));
        // Failed validation never closes the descriptor.
        assert!(connected.peer_addr().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn validated_fd_rejects_datagram_from_stream_path() {
        use std::os::fd::AsFd;

        let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let err = adopt_validated_fd(udp.as_fd()).unwrap_err();
        assert!(err.to_string().contains("not SOCK_STREAM"));
        assert!(udp.local_addr().is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn validated_fd_adopts_unix_listener() {
        use std::os::fd::AsFd;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("adopt.sock");
        let std_listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let adopted = adopt_validated_fd(std_listener.as_fd()).unwrap();
        assert!(matches!(adopted, SystemdListener::Unix(_)));
    }
}
