use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
    pub log_format: LogFormat,
    pub quiet: bool,
    max_connections: Option<usize>,
    max_file_streams: Option<usize>,
    header_read_timeout: Option<Duration>,
    response_write_timeout: Option<Duration>,
    #[cfg(feature = "tls")]
    pub tls_cert: Option<PathBuf>,
    #[cfg(feature = "tls")]
    pub tls_key: Option<PathBuf>,
}

impl Args {
    pub fn parse() -> Result<Self, String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        Self::parse_from(args)
    }

    pub fn parse_from(args: Vec<String>) -> Result<Self, String> {
        let mut bind_ip: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mut bind_port: u16 = 8000;
        let mut root: Option<PathBuf> = None;
        let mut port_from_flag = false;
        let mut public = false;
        let mut directory_listing = DirectoryListingPolicy::Disabled;
        let mut symlinks = SymlinkPolicy::Denied;
        let mut dotfiles = DotfilePolicy::Denied;
        let mut log_format = LogFormat::Text;
        let mut quiet = false;
        let mut max_connections: Option<usize> = None;
        let mut max_file_streams: Option<usize> = None;
        let mut header_read_timeout: Option<Duration> = None;
        let mut response_write_timeout: Option<Duration> = None;
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
                    i += 1;
                    let addr = args.get(i).ok_or("--bind requires an argument")?;
                    if let Ok(parsed) = addr.parse::<SocketAddr>() {
                        bind_ip = parsed.ip();
                        bind_port = parsed.port();
                    } else if let Ok(ip) = addr.parse::<IpAddr>() {
                        bind_ip = ip;
                    } else {
                        return Err(format!(
                            "invalid bind address '{}': expected HOST or HOST:PORT",
                            addr
                        ));
                    }
                }
                "--port" => {
                    i += 1;
                    let port_str = args.get(i).ok_or("--port requires an argument")?;
                    bind_port = port_str
                        .parse()
                        .map_err(|e| format!("invalid port '{}': {}", port_str, e))?;
                    port_from_flag = true;
                }
                "--addr" => {
                    i += 1;
                    let addr = args.get(i).ok_or("--addr requires an argument")?;
                    let parsed: SocketAddr = addr
                        .parse()
                        .map_err(|e| format!("invalid address '{}': {}", addr, e))?;
                    bind_ip = parsed.ip();
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
                "--write-timeout" => {
                    i += 1;
                    let val = args.get(i).ok_or("--write-timeout requires an argument")?;
                    let secs: u64 = val
                        .parse()
                        .map_err(|e| format!("invalid write-timeout '{}': {}", val, e))?;
                    response_write_timeout = Some(Duration::from_secs(secs));
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

        for pos in &positional_args {
            if let Ok(port) = pos.parse::<u16>() {
                if !port_from_flag {
                    bind_port = port;
                    port_from_flag = true;
                } else if root.is_none() {
                    root = Some(PathBuf::from(pos));
                }
            } else if root.is_none() {
                root = Some(PathBuf::from(pos));
            }
        }

        let root = root.unwrap_or_else(|| PathBuf::from("."));

        if !public && bind_ip.is_unspecified() {
            return Err(
                "binding to 0.0.0.0 requires --public to acknowledge public exposure intent"
                    .to_string(),
            );
        }

        #[cfg(feature = "tls")]
        {
            match (&tls_cert, &tls_key) {
                (Some(_), Some(_)) => {}
                (None, None) => {}
                (Some(_), None) => {
                    return Err("--tls-cert requires --tls-key to be provided".to_string());
                }
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
            log_format,
            quiet,
            max_connections,
            max_file_streams,
            header_read_timeout,
            response_write_timeout,
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

    pub fn limits(&self) -> Limits {
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
        if let Some(v) = self.response_write_timeout {
            limits.response_write_timeout = v;
        }
        limits
    }
}

pub fn print_usage() {
    eprintln!("Usage: eggserve [OPTIONS] [PORT] [DIRECTORY]");
    eprintln!();
    eprintln!("eggserve: a hardened, Rust-backed static file server");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --directory <DIR>         Root directory to serve (default: current directory)");
    eprintln!("  --bind <HOST[:PORT]>      Bind host or host:port (default: 127.0.0.1)");
    eprintln!("  --port <PORT>             Bind port (default: 8000)");
    eprintln!("  --addr <HOST:PORT>        Full socket address (overrides --bind and --port)");
    eprintln!(
        "  --public                  Acknowledge public exposure intent (required for 0.0.0.0)"
    );
    eprintln!("  --directory-listing       Enable directory listing (disabled by default)");
    eprintln!("  --follow-symlinks         Follow symlinks (denied by default)");
    eprintln!("  --allow-dotfiles          Allow dotfile serving (denied by default)");
    eprintln!("  --log-format <FORMAT>     Log format: text, json, none (default: text)");
    eprintln!("  --quiet                   Suppress startup banner except errors");
    eprintln!("  --max-connections <N>      Max concurrent connections (default: 64)");
    eprintln!("  --max-file-streams <N>     Max concurrent file streams (default: 32)");
    eprintln!("  --header-timeout <SECS>    Header read timeout in seconds (default: 10)");
    eprintln!("  --write-timeout <SECS>     Response write timeout in seconds (default: 60)");
    #[cfg(feature = "tls")]
    {
        eprintln!("  --tls-cert <PATH>          PEM certificate chain (requires --tls-key)");
        eprintln!("  --tls-key <PATH>           PEM private key (requires --tls-cert)");
    }
    eprintln!("  -h, --help                Print this help message");
    eprintln!("  -V, --version             Print version");
    eprintln!();
    eprintln!("Positional arguments:");
    eprintln!("  PORT                      Port to listen on (default: 8000)");
    eprintln!("  DIRECTORY                 Directory to serve (default: current directory)");
}

pub fn print_version() {
    println!("eggserve {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "tls")]
    #[test]
    fn tls_cert_without_key_fails() {
        let result = parse(&["--tls-cert", "/tmp/cert.pem"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("--tls-key"));
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
    fn bind_invalid_address_fails() {
        let result = parse(&["--bind", "not-an-address"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid bind address"));
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
        let limits = args.limits();
        assert_eq!(limits.max_connections, 128);
        assert_eq!(limits.max_file_streams, 64);
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
