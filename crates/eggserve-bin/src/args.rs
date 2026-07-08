use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use eggserve_core::limits::Limits;
use eggserve_core::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy, SymlinkPolicy};

pub struct Args {
    pub bind: SocketAddr,
    pub root: PathBuf,
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
    max_connections: Option<usize>,
    max_file_streams: Option<usize>,
    max_header_bytes: Option<usize>,
    max_request_target_bytes: Option<usize>,
    header_read_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    response_write_timeout: Option<Duration>,
}

impl Args {
    pub fn parse() -> Result<Self, String> {
        let mut bind_ip: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mut bind_port: u16 = 8000;
        let mut root = PathBuf::from(".");
        let mut directory_listing = DirectoryListingPolicy::Disabled;
        let mut symlinks = SymlinkPolicy::Denied;
        let mut dotfiles = DotfilePolicy::Denied;
        let mut max_connections: Option<usize> = None;
        let mut max_file_streams: Option<usize> = None;
        let mut max_header_bytes: Option<usize> = None;
        let mut max_request_target_bytes: Option<usize> = None;
        let mut header_read_timeout: Option<Duration> = None;
        let mut idle_timeout: Option<Duration> = None;
        let mut response_write_timeout: Option<Duration> = None;
        let args: Vec<String> = std::env::args().skip(1).collect();

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--bind" => {
                    i += 1;
                    let addr = args.get(i).ok_or("--bind requires an argument")?;
                    let parsed: SocketAddr = addr
                        .parse()
                        .map_err(|e| format!("invalid bind address '{}': {}", addr, e))?;
                    bind_ip = parsed.ip();
                    bind_port = parsed.port();
                }
                "--port" => {
                    i += 1;
                    let port_str = args.get(i).ok_or("--port requires an argument")?;
                    bind_port = port_str
                        .parse()
                        .map_err(|e| format!("invalid port '{}': {}", port_str, e))?;
                }
                "--public" => {
                    bind_ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
                }
                "--directory-listing" => {
                    directory_listing = DirectoryListingPolicy::Enabled;
                }
                "--follow-symlinks" => {
                    symlinks = SymlinkPolicy::Follow;
                }
                "--serve-dotfiles" => {
                    dotfiles = DotfilePolicy::Serve;
                }
                "--max-connections" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--max-connections requires an argument")?;
                    max_connections = Some(
                        val.parse()
                            .map_err(|e| format!("invalid max-connections '{}': {}", val, e))?,
                    );
                }
                "--max-file-streams" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--max-file-streams requires an argument")?;
                    max_file_streams = Some(
                        val.parse()
                            .map_err(|e| format!("invalid max-file-streams '{}': {}", val, e))?,
                    );
                }
                "--max-header-bytes" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--max-header-bytes requires an argument")?;
                    max_header_bytes = Some(
                        val.parse()
                            .map_err(|e| format!("invalid max-header-bytes '{}': {}", val, e))?,
                    );
                }
                "--max-request-target-bytes" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or("--max-request-target-bytes requires an argument")?;
                    max_request_target_bytes = Some(val.parse().map_err(|e| {
                        format!("invalid max-request-target-bytes '{}': {}", val, e)
                    })?);
                }
                "--header-timeout" => {
                    i += 1;
                    let val = args.get(i).ok_or("--header-timeout requires an argument")?;
                    let secs: u64 = val
                        .parse()
                        .map_err(|e| format!("invalid header-timeout '{}': {}", val, e))?;
                    header_read_timeout = Some(Duration::from_secs(secs));
                }
                "--idle-timeout" => {
                    i += 1;
                    let val = args.get(i).ok_or("--idle-timeout requires an argument")?;
                    let secs: u64 = val
                        .parse()
                        .map_err(|e| format!("invalid idle-timeout '{}': {}", val, e))?;
                    idle_timeout = Some(Duration::from_secs(secs));
                }
                "--write-timeout" => {
                    i += 1;
                    let val = args.get(i).ok_or("--write-timeout requires an argument")?;
                    let secs: u64 = val
                        .parse()
                        .map_err(|e| format!("invalid write-timeout '{}': {}", val, e))?;
                    response_write_timeout = Some(Duration::from_secs(secs));
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
                    root = PathBuf::from(arg);
                }
            }
            i += 1;
        }

        Ok(Args {
            bind: SocketAddr::new(bind_ip, bind_port),
            root,
            directory_listing,
            symlinks,
            dotfiles,
            max_connections,
            max_file_streams,
            max_header_bytes,
            max_request_target_bytes,
            header_read_timeout,
            idle_timeout,
            response_write_timeout,
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
        if let Some(v) = self.max_header_bytes {
            limits.max_header_bytes = v;
        }
        if let Some(v) = self.max_request_target_bytes {
            limits.max_request_target_bytes = v;
        }
        if let Some(v) = self.header_read_timeout {
            limits.header_read_timeout = v;
        }
        if let Some(v) = self.idle_timeout {
            limits.idle_timeout = v;
        }
        if let Some(v) = self.response_write_timeout {
            limits.response_write_timeout = v;
        }
        limits
    }
}

pub fn print_usage() {
    eprintln!("Usage: eggserve [OPTIONS] [DIRECTORY]");
    eprintln!();
    eprintln!("eggserve: a hardened, Rust-backed static file server");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --bind <ADDR>             Address to bind to (default: 127.0.0.1:8000)");
    eprintln!("  --port <PORT>             Port to listen on (default: 8000)");
    eprintln!("  --public                  Bind to all interfaces (0.0.0.0)");
    eprintln!("  --directory-listing       Enable directory listing (disabled by default)");
    eprintln!("  --follow-symlinks         Follow symlinks (denied by default)");
    eprintln!("  --serve-dotfiles          Serve dotfiles (denied by default)");
    eprintln!("  --max-connections <N>      Max concurrent connections (default: 64)");
    eprintln!("  --max-file-streams <N>     Max concurrent file streams (default: 32)");
    eprintln!("  --max-header-bytes <N>     Max header bytes (default: 32768)");
    eprintln!("  --max-request-target-bytes <N>  Max request target bytes (default: 8192)");
    eprintln!("  --header-timeout <SECS>    Header read timeout in seconds (default: 10)");
    eprintln!("  --idle-timeout <SECS>      Idle keep-alive timeout in seconds (default: 30)");
    eprintln!("  --write-timeout <SECS>     Response write timeout in seconds (default: 60)");
    eprintln!("  -h, --help                Print this help message");
    eprintln!("  -V, --version             Print version");
    eprintln!();
    eprintln!("Positional arguments:");
    eprintln!("  DIRECTORY                 Directory to serve (default: current directory)");
}

pub fn print_version() {
    println!("eggserve {}", env!("CARGO_PKG_VERSION"));
}
