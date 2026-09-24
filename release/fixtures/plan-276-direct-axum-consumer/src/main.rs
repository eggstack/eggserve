use axum::Router;
use eggserve_server::{RequestBodyPolicy, TowerToEggserve};

fn main() {
    let router: Router = Router::new();
    let _adapter = TowerToEggserve::with_policy(router, RequestBodyPolicy::default());
}
