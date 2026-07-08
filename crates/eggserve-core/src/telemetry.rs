//! Startup logging and telemetry.

use crate::config::ServeConfig;
use crate::policy::{DirectoryListingPolicy, DotfilePolicy, SymlinkPolicy};

pub fn log_startup(config: &ServeConfig) {
    let bind = &config.bind;
    let root = &config.root;
    let limits = &config.limits;

    println!("eggserve {}", env!("CARGO_PKG_VERSION"));
    println!("Serving root: {}", root.display());
    println!("Listening: http://{}", bind);
    println!("Methods: GET, HEAD");
    println!(
        "Directory listing: {}",
        match config.static_policy.directory_listing {
            DirectoryListingPolicy::Enabled => "enabled",
            DirectoryListingPolicy::Disabled => "disabled",
        }
    );
    println!(
        "Symlinks: {}",
        match config.static_policy.symlinks {
            SymlinkPolicy::Follow => "follow",
            SymlinkPolicy::Denied => "denied",
        }
    );
    println!(
        "Dotfiles: {}",
        match config.static_policy.dotfiles {
            DotfilePolicy::Serve => "serve",
            DotfilePolicy::Denied => "denied",
        }
    );
    println!("Max connections: {}", limits.max_connections);
    println!("Max file streams: {}", limits.max_file_streams);

    if bind.ip().is_unspecified() {
        eprintln!("WARNING: public bind enabled");
    }
    if config.static_policy.symlinks == SymlinkPolicy::Follow {
        eprintln!("WARNING: symlink following enabled");
    }
    if config.static_policy.dotfiles == DotfilePolicy::Serve {
        eprintln!("WARNING: dotfile serving enabled");
    }
}
