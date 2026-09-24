use eggserve_core::primitives::{Response, ResponseBody, StatusCode};
use eggserve_core::server::{service_fn, Request, RuntimeConfig, Server};

#[test]
fn compatibility_paths_remain_available() {
    let _runtime = RuntimeConfig::default();
    let _service = service_fn(|_request: Request| async {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Empty)
            .unwrap())
    });
    let _server_type = std::any::type_name::<Server>();
    let _static_type = std::any::type_name::<eggserve_core::server::StaticService>();
}
