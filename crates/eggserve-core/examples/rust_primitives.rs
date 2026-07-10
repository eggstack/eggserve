//! Example: using eggserve-core primitives for request handling.
//!
//! This example parses a request target, resolves it under a SecureRoot,
//! and plans a response — all without Hyper or an HTTP server.
//!
//! Run with: cargo run --example rust_primitives -p eggserve-core

use eggserve_core::primitives::{
    http::{validate_method, validate_request_body},
    resolve_and_plan, ConfinedPath, PathPolicy, ResolvedResource, SecureRoot, StaticPolicy,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = SecureRoot::new(".", StaticPolicy::safe_default())?;

    let target = "/Cargo.toml";
    let method = "GET";

    // Validate the request
    let read_only = validate_method(method)?;
    validate_request_body(None, None, 0)?;

    // Parse and resolve
    let policy = PathPolicy::default();
    let confined = ConfinedPath::parse(target, &policy)?;

    // Resolve, plan, and produce body in one call
    match resolve_and_plan(
        &root, &confined, read_only, None, // If-None-Match
        None, // If-Modified-Since
        None, // Range
        None, // If-Range
    ) {
        Ok((plan, _body)) => {
            println!("Status: {}", plan.status);
            println!("Headers:");
            for h in plan.headers.iter() {
                println!("  {}: {}", h.name, h.value);
            }
            println!("Body plan: {:?}", plan.body);
        }
        Err(e) => {
            // Check resource type for non-file cases
            let resource = root.resolve(&confined);
            match resource {
                ResolvedResource::Directory(dir) => {
                    let entries = dir.list(&root)?;
                    println!("Directory ({} entries):", entries.len());
                    for (name, is_dir) in &entries {
                        let kind = if *is_dir { "dir" } else { "file" };
                        println!("  [{kind}] {name}");
                    }
                }
                ResolvedResource::NotFound => {
                    println!("404 Not Found");
                }
                ResolvedResource::Denied(reason) => {
                    println!("403 Forbidden: {reason}");
                }
                _ => {
                    println!("Error: {e}");
                }
            }
        }
    }

    Ok(())
}
