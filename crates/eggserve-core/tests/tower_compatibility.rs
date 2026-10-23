//! Re-export check for the compatibility `http`-interop and Tower paths.
//!
//! Pins that the historical `eggserve_core::primitives::interop` and
//! `eggserve_core::server::tower` paths still resolve to the single
//! authority in `eggserve-server`, and that the adapter types are the same
//! types the direct crate exports.

#![cfg(feature = "tower")]

use eggserve_core::primitives::interop::HttpRequestBody;
use eggserve_core::server::tower::{EggserveToTower, TowerToEggserve as ModuleTowerToEggserve};
use eggserve_core::server::TowerToEggserve;
use eggserve_server::RequestBodyPolicy;

#[test]
fn historical_interop_and_tower_paths_reexport_server_authority() {
    let _body_type = std::any::type_name::<HttpRequestBody>();
    let _adapter_type = std::any::type_name::<TowerToEggserve<fn()>>();
    let _module_adapter_type = std::any::type_name::<ModuleTowerToEggserve<fn()>>();
    let _inverse_adapter_type = std::any::type_name::<EggserveToTower<()>>();
    let _policy = RequestBodyPolicy::default();
}
