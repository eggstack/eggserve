use clap::Parser;

/// eggserve: a hardened, Rust-backed static file server.
#[derive(Parser)]
#[command(name = "eggserve", version, about)]
struct Cli {
    /// Directory to serve files from.
    #[arg(default_value = ".")]
    directory: std::path::PathBuf,
}

fn main() {
    let cli = Cli::parse();
    println!(
        "eggserve (skeleton) — serving from {}",
        cli.directory.display()
    );
}
