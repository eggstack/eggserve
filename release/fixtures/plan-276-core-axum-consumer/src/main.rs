use axum::Router;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::server::TowerToEggserve;

fn main() {
    let router: Router = Router::new();
    let _adapter = TowerToEggserve::with_policy(router, RequestBodyPolicy::default());
}
