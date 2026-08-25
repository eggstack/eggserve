//! Use EggServe's public security and response-planning primitives without a socket.
//!
//! Run with: cargo run --example primitives -p eggserve-core

use eggserve_core::primitives::{
    http::{validate_method, validate_request_body},
    resolve_and_plan, ConfinedPath, PathPolicy, ResolvedResource, SecureRoot, StaticPolicy,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = SecureRoot::new(".", StaticPolicy::safe_default())?;
    let method = validate_method("GET")?;
    validate_request_body(None, None, 0)?;
    let path = ConfinedPath::parse("/Cargo.toml", &PathPolicy::default())?;

    match resolve_and_plan(&root, &path, method, None, None, None, None, None, None) {
        Ok((plan, _body)) => {
            println!("Status: {}", plan.status);
            println!("Body plan: {:?}", plan.body);
        }
        Err(error) => match root.resolve(&path) {
            ResolvedResource::Directory(_) => println!("Directory: {error}"),
            ResolvedResource::NotFound => println!("404 Not Found"),
            ResolvedResource::Denied(reason) => println!("403 Forbidden: {reason}"),
            ResolvedResource::File(_) => println!("Planning failed: {error}"),
        },
    }

    Ok(())
}
