use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use eggserve_core::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy, SymlinkPolicy};

pub struct Args {
    pub bind: SocketAddr,
    pub root: PathBuf,
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
}

impl Args {
    pub fn parse() -> Result<Self, String> {
        let mut bind_ip: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mut bind_port: u16 = 8000;
        let mut root = PathBuf::from(".");
        let mut directory_listing = DirectoryListingPolicy::Disabled;
        let mut symlinks = SymlinkPolicy::Denied;
        let mut dotfiles = DotfilePolicy::Denied;
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
        })
    }

    pub fn static_policy(&self) -> StaticPolicy {
        StaticPolicy {
            directory_listing: self.directory_listing,
            symlinks: self.symlinks,
            dotfiles: self.dotfiles,
        }
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
    eprintln!("  -h, --help                Print this help message");
    eprintln!("  -V, --version             Print version");
    eprintln!();
    eprintln!("Positional arguments:");
    eprintln!("  DIRECTORY                 Directory to serve (default: current directory)");
}

pub fn print_version() {
    println!("eggserve {}", env!("CARGO_PKG_VERSION"));
}
