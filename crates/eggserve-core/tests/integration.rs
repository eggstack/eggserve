use std::fs;

mod common;

fn extract_body_bytes(resp: &mut eggserve_core::primitives::canonical::Response) -> Vec<u8> {
    common::extract_body_bytes(&*resp)
}

use eggserve_core::policy::SymlinkPolicy;
use eggserve_core::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy};
use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme};
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body::RequestBody;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;
use eggserve_core::server::service::Service;
use eggserve_core::server::StaticService;
use std::net::SocketAddr;
use tempfile::TempDir;

fn test_connection() -> ConnectionInfo {
    ConnectionInfo {
        local_addr: Some("127.0.0.1:8000".parse::<SocketAddr>().unwrap()),
        remote_addr: Some("127.0.0.1:12345".parse::<SocketAddr>().unwrap()),
        scheme: Scheme::Http,
        tls: None,
    }
}

fn make_request(method: Method, path: &str) -> Request {
    let target = RequestTarget::parse(path).unwrap();
    let head = RequestHead::new(method, target, HttpVersion::Http11, HeaderBlock::new());
    Request::new(head, RequestBody::empty(), test_connection())
}

fn get(path: &str) -> Request {
    make_request(Method::get(), path)
}

fn head(path: &str) -> Request {
    make_request(Method::head(), path)
}

fn method_req(method: Method, path: &str) -> Request {
    make_request(method, path)
}

fn make_service(tmp: &TempDir, policy: StaticPolicy) -> StaticService {
    StaticService::builder(tmp.path())
        .policy(policy)
        .build()
        .unwrap()
}

#[tokio::test]
async fn get_existing_file_returns_200_with_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/hello.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"hello world");
}

#[tokio::test]
async fn head_existing_file_returns_200_empty_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(head("/hello.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body.len(), 0);
}

#[tokio::test]
async fn get_missing_file_returns_404() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/nonexistent.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"404 Not Found\n");
}

#[tokio::test]
async fn get_denied_dotfile_returns_403() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".env"), "SECRET_KEY=abc").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/.env")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"403 Forbidden\n");
}

#[cfg(unix)]
#[tokio::test]
async fn get_symlink_returns_403_under_safe_default() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("real.txt"), "real content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/link.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn get_directory_with_index_serves_index() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(
        tmp.path().join("subdir").join("index.html"),
        "<html>index</html>",
    )
    .unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/subdir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"<html>index</html>");
}

#[cfg(unix)]
#[tokio::test]
async fn index_final_symlink_denied_when_symlinks_denied() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(tmp.path().join("real_index.html"), "<html>real</html>").unwrap();
    std::os::unix::fs::symlink(
        tmp.path().join("real_index.html"),
        tmp.path().join("subdir").join("index.html"),
    )
    .unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/subdir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn index_final_symlink_allowed_when_follow_enabled_if_inside_root() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(tmp.path().join("real_index.html"), "<html>real</html>").unwrap();
    std::os::unix::fs::symlink(
        tmp.path().join("real_index.html"),
        tmp.path().join("subdir").join("index.html"),
    )
    .unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/subdir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"<html>real</html>");
}

#[cfg(unix)]
#[tokio::test]
async fn index_final_symlink_outside_root_denied_when_follow_enabled() {
    let tmp_root = TempDir::new().unwrap();
    let tmp_outside = TempDir::new().unwrap();
    fs::create_dir(tmp_root.path().join("subdir")).unwrap();
    fs::write(
        tmp_outside.path().join("real_index.html"),
        "<html>leaked</html>",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        tmp_outside.path().join("real_index.html"),
        tmp_root.path().join("subdir").join("index.html"),
    )
    .unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp_root, policy);

    let resp = svc.call(get("/subdir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn index_under_intermediate_symlink_denied_when_symlinks_denied() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("real_dir")).unwrap();
    fs::write(
        tmp.path().join("real_dir").join("index.html"),
        "<html>real</html>",
    )
    .unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir")).unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/link_dir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn index_under_intermediate_symlink_allowed_when_follow_enabled_if_inside_root() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("real_dir")).unwrap();
    fs::write(
        tmp.path().join("real_dir").join("index.html"),
        "<html>real</html>",
    )
    .unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir")).unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/link_dir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"<html>real</html>");
}

#[tokio::test]
async fn get_directory_without_index_returns_403_when_listing_disabled() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/subdir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn get_unsupported_method_returns_405() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc
        .call(method_req(Method::post(), "/anything"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 405);
    assert_eq!(
        resp.headers().get_first("allow").unwrap().to_str().unwrap(),
        "GET, HEAD"
    );
}

#[tokio::test]
async fn content_length_matches_file_length() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "hello").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/file.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "5"
    );
}

#[tokio::test]
async fn content_type_defaults_to_octet_stream_for_unknown_extension() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.xyz"), "data").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/file.xyz")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers()
            .get_first("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/octet-stream"
    );
}

#[tokio::test]
async fn content_type_known_extension_is_mapped() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.html"), "<html></html>").unwrap();
    fs::write(tmp.path().join("style.css"), "body{}").unwrap();
    fs::write(tmp.path().join("script.js"), "alert(1)").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/file.html")).await.unwrap();
    assert_eq!(
        resp.headers()
            .get_first("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/html; charset=utf-8"
    );

    let resp = svc.call(get("/style.css")).await.unwrap();
    assert_eq!(
        resp.headers()
            .get_first("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/css; charset=utf-8"
    );

    let resp = svc.call(get("/script.js")).await.unwrap();
    assert_eq!(
        resp.headers()
            .get_first("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/javascript; charset=utf-8"
    );
}

#[tokio::test]
async fn response_does_not_leak_absolute_root_path_on_error() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/nonexistent")).await.unwrap();
    let body = extract_body_bytes(&mut resp);
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        !body_str.contains(&tmp.path().to_string_lossy().to_string()),
        "error body should not contain absolute root path"
    );
}

#[tokio::test]
async fn nosniff_header_present() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/file.txt")).await.unwrap();
    assert_eq!(
        resp.headers()
            .get_first("x-content-type-options")
            .unwrap()
            .to_str()
            .unwrap(),
        "nosniff"
    );
}

#[tokio::test]
async fn etag_header_present() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/file.txt")).await.unwrap();
    let etag = resp
        .headers()
        .get_first("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(etag.starts_with("W/\""));
    assert!(etag.ends_with('"'));
}

#[tokio::test]
async fn last_modified_header_present() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/file.txt")).await.unwrap();
    assert!(resp.headers().get_first("last-modified").is_some());
}

#[tokio::test]
#[cfg(unix)]
async fn directory_listing_enabled_shows_entries() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(tmp.path().join("a.txt"), "a").unwrap();
    fs::write(tmp.path().join("b.txt"), "b").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("a.txt"));
    assert!(body_str.contains("b.txt"));
    assert!(body_str.contains("subdir"));
}

#[tokio::test]
#[cfg(unix)]
async fn directory_listing_escapes_html() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file with 'quotes' & ampersand"), "xss").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/")).await.unwrap();
    let body = extract_body_bytes(&mut resp);
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("file with &#x27;quotes&#x27; &amp; ampersand"));
}

#[cfg(not(windows))]
#[tokio::test]
async fn directory_listing_percent_encodes_url_significant_chars_in_href() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a?b.txt"), "x").unwrap();
    fs::write(tmp.path().join("a b.txt"), "x").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/")).await.unwrap();
    let body = extract_body_bytes(&mut resp);
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("href=\"a%3Fb.txt\""));
    assert!(body_str.contains("href=\"a%20b.txt\""));
}

#[tokio::test]
#[cfg(unix)]
async fn directory_listing_has_security_headers() {
    let tmp = TempDir::new().unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let resp = svc.call(get("/")).await.unwrap();
    assert_eq!(
        resp.headers()
            .get_first("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap(),
        "default-src 'none'; base-uri 'none'; form-action 'none'"
    );
    assert_eq!(
        resp.headers()
            .get_first("referrer-policy")
            .unwrap()
            .to_str()
            .unwrap(),
        "no-referrer"
    );
    assert_eq!(
        resp.headers()
            .get_first("x-content-type-options")
            .unwrap()
            .to_str()
            .unwrap(),
        "nosniff"
    );
}

#[tokio::test]
async fn directory_listing_does_not_include_absolute_path() {
    let tmp = TempDir::new().unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/")).await.unwrap();
    let body = extract_body_bytes(&mut resp);
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        !body_str.contains(&tmp.path().to_string_lossy().to_string()),
        "listing should not contain absolute filesystem path"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn directory_listing_head_has_no_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.txt"), "a").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(head("/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers()
            .get_first("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/html; charset=utf-8"
    );

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body.len(), 0);
}

#[tokio::test]
async fn dotfile_allowed_when_policy_permits() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".env"), "SECRET").unwrap();
    let policy = StaticPolicy {
        dotfiles: DotfilePolicy::Serve,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/.env")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"SECRET");
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_followed_when_policy_permits() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("real.txt"), "real content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/link.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"real content");
}

#[tokio::test]
async fn get_root_without_index_returns_403() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn percent_encoded_path_serves_correct_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file with spaces.txt"), "spacey").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/file%20with%20spaces.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"spacey");
}

#[tokio::test]
async fn subdir_file_served_correctly() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("a")).unwrap();
    fs::create_dir(tmp.path().join("a").join("b")).unwrap();
    fs::write(tmp.path().join("a").join("b").join("c.txt"), "nested").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/a/b/c.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"nested");
}

#[tokio::test]
async fn method_not_allowed_for_delete() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc
        .call(method_req(Method::delete(), "/file"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 405);
    assert_eq!(
        resp.headers().get_first("allow").unwrap().to_str().unwrap(),
        "GET, HEAD"
    );
}

#[tokio::test]
async fn method_not_allowed_for_patch() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc
        .call(method_req(Method::patch(), "/file"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 405);
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_missing() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let get_resp = svc.call(get("/nope")).await.unwrap();
    let head_resp = svc.call(head("/nope")).await.unwrap();
    assert_eq!(get_resp.status().as_u16(), head_resp.status().as_u16());
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_dotfile() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".hidden"), "secret").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let get_resp = svc.call(get("/.hidden")).await.unwrap();
    let head_resp = svc.call(head("/.hidden")).await.unwrap();
    assert_eq!(get_resp.status().as_u16(), head_resp.status().as_u16());
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_directory_without_index() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("emptydir")).unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let get_resp = svc.call(get("/emptydir")).await.unwrap();
    let head_resp = svc.call(head("/emptydir")).await.unwrap();
    assert_eq!(get_resp.status().as_u16(), head_resp.status().as_u16());
}

#[tokio::test]
async fn dotfile_denied_in_subdir() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("sub")).unwrap();
    fs::write(tmp.path().join("sub").join(".gitignore"), "*.o").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/sub/.gitignore")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
#[cfg(unix)]
async fn directory_listing_denies_dotfile_entries() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".hidden"), "secret").unwrap();
    fs::write(tmp.path().join("visible.txt"), "public").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        dotfiles: DotfilePolicy::Denied,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/")).await.unwrap();
    let body = extract_body_bytes(&mut resp);
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("visible.txt"));
    assert!(!body_str.contains(".hidden"));
}

#[tokio::test]
async fn large_file_returns_correct_content_length() {
    let tmp = TempDir::new().unwrap();
    let content = "x".repeat(100_000);
    fs::write(tmp.path().join("big.txt"), &content).unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let mut resp = svc.call(get("/big.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers()
            .get_first("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "100000"
    );

    let body = extract_body_bytes(&mut resp);
    assert_eq!(body.len(), 100_000);
}

#[cfg(unix)]
#[tokio::test]
async fn intermediate_symlink_denied_when_symlinks_denied() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("real_dir")).unwrap();
    fs::write(tmp.path().join("real_dir").join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir")).unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/link_dir/file.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn intermediate_symlink_inside_root_allowed_when_follow_enabled() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("real_dir")).unwrap();
    fs::write(tmp.path().join("real_dir").join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir")).unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp, policy);

    let mut resp = svc.call(get("/link_dir/file.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body = extract_body_bytes(&mut resp);
    assert_eq!(body, b"content");
}

#[cfg(unix)]
#[tokio::test]
async fn intermediate_symlink_escape_denied_when_follow_enabled() {
    let tmp_root = TempDir::new().unwrap();
    let tmp_outside = TempDir::new().unwrap();
    fs::create_dir(tmp_outside.path().join("secret_dir")).unwrap();
    fs::write(
        tmp_outside.path().join("secret_dir").join("file.txt"),
        "leaked",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        tmp_outside.path().join("secret_dir"),
        tmp_root.path().join("out"),
    )
    .unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp_root, policy);

    let resp = svc.call(get("/out/file.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn final_symlink_outside_root_denied_when_follow_enabled() {
    let tmp_root = TempDir::new().unwrap();
    let tmp_outside = TempDir::new().unwrap();
    fs::write(tmp_outside.path().join("secret.txt"), "leaked").unwrap();
    std::os::unix::fs::symlink(
        tmp_outside.path().join("secret.txt"),
        tmp_root.path().join("escape.txt"),
    )
    .unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp_root, policy);

    let resp = svc.call(get("/escape.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn nested_intermediate_symlink_denied() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("a")).unwrap();
    fs::create_dir(tmp.path().join("b")).unwrap();
    fs::write(tmp.path().join("b").join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("b"), tmp.path().join("a").join("link_b")).unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/a/link_b/file.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn get_put_delete_patch_all_405() {
    let tmp = TempDir::new().unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    for m in [Method::put(), Method::delete(), Method::patch()] {
        let resp = svc.call(method_req(m.clone(), "/file")).await.unwrap();
        assert_eq!(
            resp.status().as_u16(),
            405,
            "{} should return 405",
            m.as_str()
        );
    }
}

#[tokio::test]
async fn double_encoded_dotdot_is_rejected() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/%252e%252e/hello.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn double_encoded_slash_is_treated_as_literal() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/%252f%252e%252e/hello.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn single_encoded_dotdot_is_rejected() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/%2e%2e/hello.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn encoded_dotfile_denied() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".env"), "secret").unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/%2eenv")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_outside_root_denied_even_when_follow_enabled() {
    let tmp_root = TempDir::new().unwrap();
    let tmp_outside = TempDir::new().unwrap();
    fs::write(tmp_outside.path().join("secret.txt"), "leaked").unwrap();
    std::os::unix::fs::symlink(
        tmp_outside.path().join("secret.txt"),
        tmp_root.path().join("escape.txt"),
    )
    .unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let svc = make_service(&tmp_root, policy);

    let resp = svc.call(get("/escape.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn hidden_index_name_is_not_considered_index() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(
        tmp.path().join("subdir").join(".index.html"),
        "secret index",
    )
    .unwrap();
    let svc = make_service(&tmp, StaticPolicy::safe_default());

    let resp = svc.call(get("/subdir/")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_symlink_swap_stress() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    let tmp_root = TempDir::new().unwrap();
    let tmp_outside = TempDir::new().unwrap();

    fs::write(tmp_root.path().join("safe.txt"), "safe").unwrap();
    fs::write(tmp_outside.path().join("secret.txt"), "LEAKED").unwrap();

    std::os::unix::fs::symlink(
        tmp_root.path().join("safe.txt"),
        tmp_root.path().join("link.txt"),
    )
    .unwrap();

    let policy = StaticPolicy::safe_default();
    let outside_secret = tmp_outside.path().join("secret.txt");
    let link_path = tmp_root.path().join("link.txt");
    let safe_target = tmp_root.path().join("safe.txt");
    let leaked = Arc::new(AtomicBool::new(false));

    const ITERS: usize = 100;
    const THREADS: usize = 4;

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let root = tmp_root.path().to_path_buf();
            let policy = policy.clone();
            let outside_secret = outside_secret.clone();
            let link_path = link_path.clone();
            let safe_target = safe_target.clone();
            let leaked = Arc::clone(&leaked);
            thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                for i in 0..ITERS {
                    let swap_to_outside = i % 2 == 0;

                    let link_tmp = link_path.with_file_name(format!("link.{}.{}.tmp", t, i));
                    std::os::unix::fs::symlink(
                        if swap_to_outside {
                            &outside_secret
                        } else {
                            &safe_target
                        },
                        &link_tmp,
                    )
                    .unwrap();
                    std::fs::rename(&link_tmp, &link_path).unwrap();

                    let svc = StaticService::builder(&root)
                        .policy(policy.clone())
                        .build()
                        .unwrap();
                    let req = make_request(Method::get(), "/link.txt");
                    let mut resp = rt.block_on(svc.call(req)).unwrap();
                    let status = resp.status().as_u16();

                    let body = extract_body_bytes(&mut resp);
                    let body_str = String::from_utf8_lossy(&body);

                    if body_str.contains("LEAKED") {
                        leaked.store(true, Ordering::SeqCst);
                    }

                    assert!(
                        status == 403 || status == 200,
                        "unexpected status {} on iteration {} (swapped_to_outside={})",
                        status,
                        i,
                        swap_to_outside,
                    );
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    assert!(
        !leaked.load(Ordering::SeqCst),
        "symlink escape succeeded under concurrent swap stress — content from outside root was served"
    );
}
