use std::net::SocketAddr;
use std::path::Path;

pub fn log_startup(bind: &SocketAddr, root: &Path) {
    println!("eggserve starting");
    println!("Serving root: {}", root.display());
    println!("Listening: http://{}", bind);
    println!("Methods: GET, HEAD");
    println!("Directory listing: disabled");
    println!("Symlinks: denied");
    println!("Dotfiles: denied");
}
