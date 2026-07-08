use crate::config::ServeConfig;

pub fn log_startup(config: &ServeConfig) {
    let bind = &config.bind;
    let root = &config.root;
    let limits = &config.limits;

    println!("eggserve starting");
    println!("Serving root: {}", root.display());
    println!("Listening: http://{}", bind);
    println!("Methods: GET, HEAD");
    println!(
        "Directory listing: {}",
        match config.static_policy.directory_listing {
            crate::policy::DirectoryListingPolicy::Enabled => "enabled",
            crate::policy::DirectoryListingPolicy::Disabled => "disabled",
        }
    );
    println!(
        "Symlinks: {}",
        match config.static_policy.symlinks {
            crate::policy::SymlinkPolicy::Follow => "follow",
            crate::policy::SymlinkPolicy::Denied => "denied",
        }
    );
    println!(
        "Dotfiles: {}",
        match config.static_policy.dotfiles {
            crate::policy::DotfilePolicy::Serve => "serve",
            crate::policy::DotfilePolicy::Denied => "denied",
        }
    );
    println!("Max connections: {}", limits.max_connections);
    println!("Max file streams: {}", limits.max_file_streams);
    println!("Header timeout: {}s", limits.header_read_timeout.as_secs());
    println!("Idle timeout: {}s", limits.idle_timeout.as_secs());
    println!(
        "Write timeout: {}s",
        limits.response_write_timeout.as_secs()
    );
}
