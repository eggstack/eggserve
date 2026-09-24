use eggserve_server::{
    AdmissionOwner, AdmissionOwnership, H1PolicyOwnership, Http1RequestTargetMode, PolicyOwner,
    RuntimeConfig,
};

#[test]
fn defaults_remain_eggserve_owned_and_bounded() {
    let config = RuntimeConfig::default();
    assert_eq!(config.http1_request_target_mode, Http1RequestTargetMode::OriginOnly);
    assert_eq!(config.policy_ownership, H1PolicyOwnership::eggserve_owned());
    assert_eq!(config.admission_ownership, AdmissionOwnership::eggserve_owned());
    assert_eq!(config.policy_ownership.handler_deadline, PolicyOwner::EggServe);
    assert_eq!(config.policy_ownership.global_request_body_ceiling, PolicyOwner::EggServe);
    assert_eq!(config.policy_ownership.request_target_ceiling, PolicyOwner::EggServe);
    assert_eq!(config.admission_ownership.service_calls, AdmissionOwner::EggServe);
    assert_eq!(config.admission_ownership.tunnels, AdmissionOwner::EggServe);
    assert_eq!(config.max_request_body_bytes, 0);
    assert!(config.max_request_target_bytes > 0);
    assert!(config.max_headers > 0);
    assert!(config.max_header_bytes > 0);
}
