use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use eggserve_core::limits::Limits;
use eggserve_core::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy, SymlinkPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
    None,
}

#[derive(Debug)]
pub struct Args {
    pub bind: SocketAddr,
    pub root: PathBuf,
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
    pub default_content_type: String,
    pub extra_response_headers: Vec<(String, String)>,
    pub log_format: LogFormat,
    pub quiet: bool,
    max_connections: Option<usize>,
    max_file_streams: Option<usize>,
    header_read_timeout: Option<Duration>,
    connection_total_timeout: Option<Duration>,
    handler_timeout: Option<Duration>,
    body_read_timeout: Option<Duration>,
    #[cfg(feature = "tls")]
    pub tls_cert: Option<PathBuf>,
    #[cfg(feature = "tls")]
    pub tls_key: Option<PathBuf>,
}

fn split_bind_host_port(value: &str) -> Option<(&str, Option<u16>)> {
    if value.starts_with('[') {
        let end = value.find(']')?;
        let host = &value[1..end];
        let rest = &value[end + 1..];
        if rest.is_empty() {
            return Some((host, None));
        }
        let port = rest.strip_prefix(':')?.parse().ok()?;
        return Some((host, Some(port)));
    }
    if value.matches(':').count() > 1 {
        return None;
    }
    match value.split_once(':') {
        Some((host, port)) if !host.is_empty() => Some((host, Some(port.parse().ok()?))),
        None if !value.is_empty() => Some((value, None)),
        _ => None,
    }
}

impl Args {
    pub fn parse() -> Result<Self, String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        Self::parse_from(args)
    }

    pub fn parse_from(args: Vec<String>) -> Result<Self, String> {
        let mut bind_host = Ipv4Addr::LOCALHOST.to_string();
        let mut bind_port: u16 = 8000;
        let mut root: Option<PathBuf> = None;
        let mut port_from_flag = false;
        let mut bind_seen = false;
        let mut addr_seen = false;
        let mut public = false;
        let mut directory_listing = DirectoryListingPolicy::Disabled;
        let mut symlinks = SymlinkPolicy::Denied;
        let mut dotfiles = DotfilePolicy::Denied;
        let mut log_format = LogFormat::Text;
        let mut quiet = false;
        let mut max_connections: Option<usize> = None;
        let mut max_file_streams: Option<usize> = None;
        let mut header_read_timeout: Option<Duration> = None;
        let mut connection_total_timeout: Option<Duration> = None;
        let mut handler_timeout: Option<Duration> = None;
        let mut body_read_timeout: Option<Duration> = None;
        let mut default_content_type = "application/octet-stream".to_string();
        let mut extra_response_headers = Vec::new();
        #[cfg(feature = "tls")]
        let mut tls_cert: Option<PathBuf> = None;
        #[cfg(feature = "tls")]
        let mut tls_key: Option<PathBuf> = None;
        let mut positional_args: Vec<String> = Vec::new();

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--directory" => {
                    i += 1;
                    let dir = args.get(i).ok_or("--directory requires an argument")?;
                    root = Some(PathBuf::from(dir));
                }
                "--bind" => {
                    if addr_seen {
                        return Err("--bind and --addr cannot be used together".to_string());
                    }
                    bind_seen = true;
                    i += 1;
                    let addr = args.get(i).ok_or("--bind requires an argument")?;
                    if let Ok(parsed) = addr.parse::<SocketAddr>() {
                        bind_host = parsed.ip().to_string();
                        bind_port = parsed.port();
                        port_from_flag = true;
                    } else if let Ok(ip) = addr.parse::<IpAddr>() {
                        bind_host = ip.to_string();
                    } else if let Some((host, port)) = split_bind_host_port(addr) {
                        bind_host = host.to_string();
                        if let Some(port) = port {
                            bind_port = port;
                            port_from_flag = true;
                        }
                    } else {
                        return Err(format!(
                            "invalid bind address '{}': expected HOST or HOST:PORT",
                            addr
                        ));
                    }
                }
                "--port" => {
                    if addr_seen {
                        return Err("--port cannot be used with --addr (the port is set in --addr HOST:PORT)".into());
                    }
                    i += 1;
                    let port_str = args.get(i).ok_or("--port requires an argument")?;
                    bind_port = port_str
                        .parse()
                        .map_err(|e| format!("invalid port '{}': {}", port_str, e))?;
                    port_from_flag = true;
                }
                "--addr" => {
                    if bind_seen {
                        return Err("--bind and --addr cannot be used together".to_string());
                    }
                    addr_seen = true;
                    i += 1;
                    let addr = args.get(i).ok_or("--addr requires an argument")?;
                    let parsed: SocketAddr = addr
                        .parse()
                        .map_err(|e| format!("invalid address '{}': {}", addr, e))?;
                    bind_host = parsed.ip().to_string();
                    bind_port = parsed.port();
                    port_from_flag = true;
                }
                "--public" => {
                    public = true;
                }
                "--directory-listing" => {
                    directory_listing = DirectoryListingPolicy::Enabled;
                }
                "--follow-symlinks" => {
                    symlinks = SymlinkPolicy::Follow;
                }
                "--allow-dotfiles" => {
                    dotfiles = DotfilePolicy::Serve;
                }
                "--log-format" => {
                    i += 1;
                    let fmt = args.get(i).ok_or("--log-format requires an argument")?;
                    log_format = match fmt.as_str() {
                        "text" => LogFormat::Text,
                        "json" => LogFormat::Json,
                        "none" => LogFormat::None,
                        other => {
                            return Err(format!(
                                "invalid log format '{}': expected text, json, or none",
                                other
                            ))
                        }
                    };
                }
                "--quiet" => {
                    quiet = true;
                }
                "--max-connections" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--max-connections requires an argument")?;
                    let parsed: usize = val
                        .parse()
                        .map_err(|e| format!("invalid max-connections '{}': {}", val, e))?;
                    if parsed == 0 {
                        return Err("--max-connections must be greater than 0".to_string());
                    }
                    max_connections = Some(parsed);
                }
                "--max-file-streams" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--max-file-streams requires an argument")?;
                    let parsed: usize = val
                        .parse()
                        .map_err(|e| format!("invalid max-file-streams '{}': {}", val, e))?;
                    if parsed == 0 {
                        return Err("--max-file-streams must be greater than 0".to_string());
                    }
                    max_file_streams = Some(parsed);
                }
                "--header-timeout" => {
                    i += 1;
                    let val = args.get(i).ok_or("--header-timeout requires an argument")?;
                    let secs: u64 = val
                        .parse()
                        .map_err(|e| format!("invalid header-timeout '{}': {}", val, e))?;
                    header_read_timeout = Some(Duration::from_secs(secs));
                }
                "--connection-total-timeout" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--connection-total-timeout requires an argument")?;
                    let secs: u64 = val.parse().map_err(|e| {
                        format!("invalid connection-total-timeout '{}': {}", val, e)
                    })?;
                    connection_total_timeout = Some(Duration::from_secs(secs));
                }
                "--handler-timeout" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--handler-timeout requires an argument")?;
                    let secs: u64 = val
                        .parse()
                        .map_err(|e| format!("invalid handler-timeout '{}': {}", val, e))?;
                    handler_timeout = Some(Duration::from_secs(secs));
                }
                "--body-read-timeout" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--body-read-timeout requires an argument")?;
                    let secs: u64 = val
                        .parse()
                        .map_err(|e| format!("invalid body-read-timeout '{}': {}", val, e))?;
                    body_read_timeout = Some(Duration::from_secs(secs));
                }
                "--content-type" => {
                    i += 1;
                    default_content_type = args
                        .get(i)
                        .ok_or("--content-type requires an argument")?
                        .clone();
                }
                "-H" | "--header" => {
                    i += 1;
                    let name = args.get(i).ok_or("--header requires NAME and VALUE")?;
                    i += 1;
                    let value = args.get(i).ok_or("--header requires NAME and VALUE")?;
                    extra_response_headers.push((name.clone(), value.clone()));
                }
                #[cfg(feature = "tls")]
                "--tls-cert" => {
                    i += 1;
                    let path = args.get(i).ok_or("--tls-cert requires an argument")?;
                    tls_cert = Some(PathBuf::from(path));
                }
                #[cfg(feature = "tls")]
                "--tls-key" => {
                    i += 1;
                    let path = args.get(i).ok_or("--tls-key requires an argument")?;
                    tls_key = Some(PathBuf::from(path));
                }
                #[cfg(not(feature = "tls"))]
                "--tls-cert" | "--tls-key" => {
                    return Err(format!(
                        "{} requires the CLI to be built with the 'tls' feature",
                        args[i]
                    ));
                }
                "--help" | "-h" => {
                    return Err("help".to_string());
                }
                "--version" | "-V" => {
                    return Err("version".to_string());
                }
                arg if arg.starts_with('-') => {
                    return Err(format!("unknown flag: {}", arg));
                }
                arg => {
                    positional_args.push(arg.to_string());
                }
            }
            i += 1;
        }

        for pos in positional_args {
            if !port_from_flag {
                if let Ok(port) = pos.parse::<u16>() {
                    bind_port = port;
                    port_from_flag = true;
                    continue;
                }
            }

            if root.is_none() {
                root = Some(PathBuf::from(pos));
            } else {
                return Err(format!("too many positional arguments: '{pos}'"));
            }
        }

        let root = root.unwrap_or_else(|| PathBuf::from("."));

        let bind_ip = (bind_host.as_str(), bind_port)
            .to_socket_addrs()
            .map_err(|e| format!("failed to resolve bind host '{}': {}", bind_host, e))?
            .next()
            .ok_or_else(|| format!("bind host '{}' did not resolve", bind_host))?
            .ip();

        if !public && bind_ip.is_unspecified() {
            return Err(format!(
                "binding to {} requires --public to acknowledge public exposure intent",
                bind_ip
            ));
        }

        eggserve_core::config::validate_static_metadata(
            &default_content_type,
            &extra_response_headers,
        )?;

        #[cfg(feature = "tls")]
        {
            match (&tls_cert, &tls_key) {
                (Some(_), Some(_)) => {}
                (None, None) => {}
                (Some(cert), None) => tls_key = Some(cert.clone()),
                (None, Some(_)) => {
                    return Err("--tls-key requires --tls-cert to be provided".to_string());
                }
            }
        }

        Ok(Args {
            bind: SocketAddr::new(bind_ip, bind_port),
            root,
            directory_listing,
            symlinks,
            dotfiles,
            default_content_type,
            extra_response_headers,
            log_format,
            quiet,
            max_connections,
            max_file_streams,
            header_read_timeout,
            connection_total_timeout,
            handler_timeout,
            body_read_timeout,
            #[cfg(feature = "tls")]
            tls_cert,
            #[cfg(feature = "tls")]
            tls_key,
        })
    }

    pub fn static_policy(&self) -> StaticPolicy {
        StaticPolicy {
            directory_listing: self.directory_listing,
            symlinks: self.symlinks,
            dotfiles: self.dotfiles,
        }
    }

    pub fn limits(&self) -> Result<Limits, String> {
        let mut limits = Limits::default();
        if let Some(v) = self.max_connections {
            limits.max_connections = v;
        }
        if let Some(v) = self.max_file_streams {
            limits.max_file_streams = v;
        }
        if let Some(v) = self.header_read_timeout {
            limits.header_read_timeout = v;
        }
        if let Some(v) = self.connection_total_timeout {
            limits.connection_total_timeout = v;
        }
        if let Some(v) = self.handler_timeout {
            limits.handler_timeout = v;
        }
        if let Some(v) = self.body_read_timeout {
            limits.body_read_timeout = v;
        }
        limits
            .validate()
            .map_err(|errs| {
                errs.iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .map(|()| limits)
    }
}

pub fn print_usage() {
    println!("Usage: eggserve [OPTIONS] [PORT] [DIRECTORY]");
    println!();
    println!("eggserve: a hardened, Rust-backed static file server");
    println!();
    println!("Options:");
    println!("  --directory <DIR>         Root directory to serve (default: current directory)");
    println!("  --bind <HOST[:PORT]>      Bind host or host:port (default: 127.0.0.1)");
    println!("  --port <PORT>             Bind port (default: 8000)");
    println!("  --addr <HOST:PORT>        Full socket address (cannot combine with --bind)");
    println!(
        "  --public                  Acknowledge public exposure intent (required for 0.0.0.0 or ::)"
    );
    println!("  --directory-listing       Enable directory listing (disabled by default)");
    println!("  --follow-symlinks         Follow symlinks (denied by default)");
    println!("  --allow-dotfiles          Allow dotfile serving (denied by default)");
    println!("  --log-format <FORMAT>     Log format: text, json, none (default: text)");
    println!("  --quiet                   Suppress routine informational output (warn/error only)");
    println!("  --max-connections <N>      Max concurrent connections (default: 64)");
    println!("  --max-file-streams <N>     Max concurrent file streams (default: 32)");
    println!("  --header-timeout <SECS>    Header read timeout in seconds (default: 10)");
    println!("  --connection-total-timeout <SECS>  Total connection lifetime timeout in seconds (default: 60)");
    println!("  --handler-timeout <SECS>   Handler invocation timeout in seconds (default: 30)");
    println!("  --body-read-timeout <SECS> Request body read timeout in seconds (default: 30)");
    println!("  --content-type <TYPE>      Fallback MIME type for unknown extensions");
    println!("  -H, --header <NAME> <VALUE>  Extra header for static 200 responses (repeatable)");
    #[cfg(feature = "tls")]
    {
        println!("  --tls-cert <PATH>          PEM certificate chain (enables TLS)");
        println!("  --tls-key <PATH>           PEM private key (defaults to --tls-cert)");
    }
    println!("  -h, --help                Print this help message");
    println!("  -V, --version             Print version");
    println!();
    println!("Positional arguments:");
    println!("  PORT                      Port to listen on (default: 8000)");
    println!("  DIRECTORY                 Directory to serve (default: current directory)");
    println!();
    println!("Positional slots are PORT followed by DIRECTORY. Explicit port sources");
    println!("(--port, --addr, or --bind HOST:PORT) occupy PORT; --bind HOST leaves");
    println!("PORT available. Once PORT is occupied, the next positional is DIRECTORY");
    println!("verbatim, including numeric names. --bind and --addr cannot be combined.");
}

pub fn print_version() {
    println!("eggserve {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "tls")]
    #[test]
    fn tls_cert_without_key_uses_combined_pem() {
        let result = parse(&["--tls-cert", "/tmp/cert.pem"]);
        let args = result.unwrap();
        assert_eq!(args.tls_cert, Some(PathBuf::from("/tmp/cert.pem")));
        assert_eq!(args.tls_key, Some(PathBuf::from("/tmp/cert.pem")));
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_key_without_cert_fails() {
        let result = parse(&["--tls-key", "/tmp/key.pem"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("--tls-cert"));
    }

    #[cfg(feature = "tls")]
    #[test]
    fn no_tls_args_parses_successfully_as_plaintext_mode() {
        let result = parse(&[]);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert!(args.tls_cert.is_none());
        assert!(args.tls_key.is_none());
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_cert_and_key_parse_successfully() {
        let result = parse(&["--tls-cert", "/tmp/cert.pem", "--tls-key", "/tmp/key.pem"]);
        assert!(result.is_ok());
        let args = result.unwrap();
        assert_eq!(args.tls_cert, Some(PathBuf::from("/tmp/cert.pem")));
        assert_eq!(args.tls_key, Some(PathBuf::from("/tmp/key.pem")));
    }

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse_from(args.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn default_config_binds_loopback_8000() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.bind, "127.0.0.1:8000".parse().unwrap());
        assert_eq!(args.root, PathBuf::from("."));
    }

    #[test]
    fn positional_port_is_parsed() {
        let args = parse(&["9000"]).unwrap();
        assert_eq!(args.bind.port(), 9000);
    }

    #[test]
    fn positional_directory_is_parsed() {
        let args = parse(&["public"]).unwrap();
        assert_eq!(args.root, PathBuf::from("public"));
    }

    #[test]
    fn positional_port_and_directory() {
        let args = parse(&["9000", "public"]).unwrap();
        assert_eq!(args.bind.port(), 9000);
        assert_eq!(args.root, PathBuf::from("public"));
    }

    #[test]
    fn positional_port_and_numeric_directory() {
        let args = parse(&["8000", "1234"]).unwrap();
        assert_eq!(args.bind.port(), 8000);
        assert_eq!(args.root, PathBuf::from("1234"));
    }

    #[test]
    fn explicit_port_and_numeric_directory() {
        let args = parse(&["--port", "9000", "1234"]).unwrap();
        assert_eq!(args.bind.port(), 9000);
        assert_eq!(args.root, PathBuf::from("1234"));
    }

    #[test]
    fn explicit_addr_and_numeric_directory() {
        let args = parse(&["--addr", "127.0.0.1:9000", "1234"]).unwrap();
        assert_eq!(args.bind, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(args.root, PathBuf::from("1234"));
    }

    #[test]
    fn directory_flag_sets_root() {
        let args = parse(&["--directory", "mydir"]).unwrap();
        assert_eq!(args.root, PathBuf::from("mydir"));
    }

    #[test]
    fn addr_overrides_bind_and_port() {
        let args = parse(&["--addr", "0.0.0.0:9090", "--public"]).unwrap();
        assert_eq!(args.bind, "0.0.0.0:9090".parse().unwrap());
    }

    #[test]
    fn bind_and_port_separate_flags() {
        let args = parse(&["--bind", "192.168.1.1:3000"]).unwrap();
        assert_eq!(args.bind, "192.168.1.1:3000".parse().unwrap());
    }

    #[test]
    fn bind_host_only_preserves_default_port() {
        let args = parse(&["--bind", "192.168.1.1"]).unwrap();
        assert_eq!(args.bind.ip(), "192.168.1.1".parse::<IpAddr>().unwrap());
        assert_eq!(args.bind.port(), 8000);
    }

    #[test]
    fn numeric_directory_does_not_override_explicit_bind_port() {
        let args = parse(&["--bind", "127.0.0.1:3000", "9000"]).unwrap();
        assert_eq!(args.bind, "127.0.0.1:3000".parse().unwrap());
        assert_eq!(args.root, PathBuf::from("9000"));
    }

    #[test]
    fn host_only_bind_allows_positional_port_and_numeric_directory() {
        let args = parse(&["--bind", "127.0.0.1", "9000", "1234"]).unwrap();
        assert_eq!(args.bind, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(args.root, PathBuf::from("1234"));
    }

    #[test]
    fn numeric_directory_flag_leaves_positional_port_available() {
        let args = parse(&["--directory", "1234", "9000"]).unwrap();
        assert_eq!(args.bind.port(), 9000);
        assert_eq!(args.root, PathBuf::from("1234"));
    }

    #[test]
    fn single_numeric_positional_remains_a_port() {
        let args = parse(&["1234"]).unwrap();
        assert_eq!(args.bind.port(), 1234);
        assert_eq!(args.root, PathBuf::from("."));
    }

    #[test]
    fn excess_positional_arguments_are_rejected() {
        let result = parse(&["8000", "public", "extra"]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("too many positional arguments"));
    }

    #[test]
    fn bind_and_addr_conflict_is_rejected() {
        let result = parse(&["--bind", "127.0.0.1", "--addr", "127.0.0.1:9000"]);
        assert!(result.unwrap_err().contains("cannot be used together"));
    }

    #[test]
    fn ipv6_unspecified_bind_error_names_actual_address() {
        let result = parse(&["--bind", "::"]);
        let error = result.unwrap_err();
        assert!(error.contains("::"));
        assert!(error.contains("--public"));
    }

    #[test]
    fn bind_invalid_address_fails() {
        let result = parse(&["--bind", "not-an-address"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to resolve bind host"));
    }

    #[test]
    fn hostname_bind_resolves_once() {
        let args = parse(&["--bind", "localhost", "0"]).unwrap();
        assert!(args.bind.ip().is_loopback());
        assert_eq!(args.bind.port(), 0);
    }

    #[test]
    fn static_metadata_options_preserve_order() {
        let args = parse(&[
            "--content-type",
            "text/x-unknown",
            "-H",
            "X-One",
            "a",
            "--header",
            "X-Two",
            "b",
        ])
        .unwrap();
        assert_eq!(args.default_content_type, "text/x-unknown");
        assert_eq!(
            args.extra_response_headers,
            [("X-One".into(), "a".into()), ("X-Two".into(), "b".into())]
        );
    }

    #[test]
    fn unsafe_static_metadata_is_rejected() {
        assert!(parse(&["--content-type", "text/plain\r\nX-Leak: yes"]).is_err());
        assert!(parse(&["-H", "Content-Length", "1"]).is_err());
    }

    #[test]
    fn public_flag_allows_unspecified_bind() {
        let args = parse(&["--addr", "0.0.0.0:8000", "--public"]).unwrap();
        assert!(args.bind.ip().is_unspecified());
    }

    #[test]
    fn public_bind_without_public_flag_fails() {
        let result = parse(&["--addr", "0.0.0.0:8000"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("--public"));
    }

    #[test]
    fn unsafe_flags_update_policy() {
        let args = parse(&[
            "--directory-listing",
            "--follow-symlinks",
            "--allow-dotfiles",
        ])
        .unwrap();
        assert_eq!(args.directory_listing, DirectoryListingPolicy::Enabled);
        assert_eq!(args.symlinks, SymlinkPolicy::Follow);
        assert_eq!(args.dotfiles, DotfilePolicy::Serve);
    }

    #[test]
    fn invalid_port_fails() {
        let result = parse(&["--port", "99999"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid port"));
    }

    #[test]
    fn invalid_addr_fails() {
        let result = parse(&["--addr", "not-an-address"]);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_flag_fails() {
        let result = parse(&["--bogus"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown flag"));
    }

    #[test]
    fn help_returns_help_error() {
        let result = parse(&["--help"]);
        assert_eq!(result.unwrap_err(), "help");
    }

    #[test]
    fn version_returns_version_error() {
        let result = parse(&["--version"]);
        assert_eq!(result.unwrap_err(), "version");
    }

    #[test]
    fn quiet_flag_is_set() {
        let args = parse(&["--quiet"]).unwrap();
        assert!(args.quiet);
    }

    #[test]
    fn log_format_none_is_set() {
        let args = parse(&["--log-format", "none"]).unwrap();
        assert_eq!(args.log_format, LogFormat::None);
    }

    #[test]
    fn log_format_json_is_set() {
        let args = parse(&["--log-format", "json"]).unwrap();
        assert_eq!(args.log_format, LogFormat::Json);
    }

    #[test]
    fn invalid_log_format_fails() {
        let result = parse(&["--log-format", "xml"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid log format"));
    }

    #[test]
    fn static_policy_reflects_flags() {
        let args = parse(&["--directory-listing", "--allow-dotfiles"]).unwrap();
        let policy = args.static_policy();
        assert_eq!(policy.directory_listing, DirectoryListingPolicy::Enabled);
        assert_eq!(policy.dotfiles, DotfilePolicy::Serve);
        assert_eq!(policy.symlinks, SymlinkPolicy::Denied);
    }

    #[test]
    fn limits_override_defaults() {
        let args = parse(&["--max-connections", "128", "--max-file-streams", "64"]).unwrap();
        let limits = args.limits().unwrap();
        assert_eq!(limits.max_connections, 128);
        assert_eq!(limits.max_file_streams, 64);
    }

    #[test]
    fn limits_validates_zero_values() {
        let args = parse(&[]).unwrap();
        let mut limits = args.limits().unwrap();
        limits.max_connections = 0;
        assert!(limits.validate().is_err());
    }

    #[test]
    fn max_connections_zero_fails() {
        let result = parse(&["--max-connections", "0"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("--max-connections"));
    }

    #[test]
    fn max_file_streams_zero_fails() {
        let result = parse(&["--max-file-streams", "0"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("--max-file-streams"));
    }
}
