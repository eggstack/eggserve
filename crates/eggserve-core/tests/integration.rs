use std::fs;
use std::sync::Arc;

use eggserve_core::config::{ServeConfig, ServeState};
#[cfg(unix)]
use eggserve_core::policy::SymlinkPolicy;
use eggserve_core::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy};
use eggserve_core::service::handle_request;
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use tempfile::TempDir;

fn make_state(tmp: &TempDir, policy: StaticPolicy) -> ServeState {
    let config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        static_policy: policy,
        ..ServeConfig::default()
    });
    ServeState::new(config).unwrap()
}

fn get(path: &str) -> Request<http_body_util::Empty<Bytes>> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(http_body_util::Empty::new())
        .unwrap()
}

fn head(path: &str) -> Request<http_body_util::Empty<Bytes>> {
    Request::builder()
        .method(Method::HEAD)
        .uri(path)
        .body(http_body_util::Empty::new())
        .unwrap()
}

fn method(method: Method, path: &str) -> Request<http_body_util::Empty<Bytes>> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(http_body_util::Empty::new())
        .unwrap()
}

async fn body_bytes<B>(resp: hyper::Response<B>) -> Bytes
where
    B: hyper::body::Body + Send,
    B::Error: std::fmt::Debug,
{
    use http_body_util::BodyExt;
    resp.into_body().collect().await.unwrap().to_bytes()
}

#[tokio::test]
async fn get_existing_file_returns_200_with_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/hello.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "hello world");
}

#[tokio::test]
async fn head_existing_file_returns_200_empty_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(head("/hello.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body.len(), 0);
}

#[tokio::test]
async fn get_missing_file_returns_404() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/nonexistent.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let body = body_bytes(resp).await;
    assert_eq!(body, "404 Not Found\n");
}

#[tokio::test]
async fn get_denied_dotfile_returns_403() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".env"), "SECRET_KEY=abc").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/.env"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let body = body_bytes(resp).await;
    assert_eq!(body, "403 Forbidden\n");
}

#[cfg(unix)]
#[tokio::test]
async fn get_symlink_returns_403_under_safe_default() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("real.txt"), "real content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/link.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/subdir"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "<html>index</html>");
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
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/subdir"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/subdir"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp).await;
    assert_eq!(body, "<html>real</html>");
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
    let state = make_state(&tmp_root, policy);

    let resp = handle_request(get("/subdir"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/link_dir"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/link_dir"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp).await;
    assert_eq!(body, "<html>real</html>");
}

#[tokio::test]
async fn get_directory_without_index_returns_403_when_listing_disabled() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/subdir"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_unsupported_method_returns_405() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(method(Method::POST, "/anything"), &state).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(resp.headers().get("allow").unwrap(), "GET, HEAD");
}

#[tokio::test]
async fn content_length_matches_file_length() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "hello").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-length").unwrap(), "5");
}

#[tokio::test]
async fn content_type_defaults_to_octet_stream_for_unknown_extension() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.xyz"), "data").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.xyz"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/octet-stream"
    );
}

#[tokio::test]
async fn content_type_known_extension_is_mapped() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.html"), "<html></html>").unwrap();
    fs::write(tmp.path().join("style.css"), "body{}").unwrap();
    fs::write(tmp.path().join("script.js"), "alert(1)").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.html"), &state).await;
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );

    let resp = handle_request(get("/style.css"), &state).await;
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/css; charset=utf-8"
    );

    let resp = handle_request(get("/script.js"), &state).await;
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/javascript; charset=utf-8"
    );
}

#[tokio::test]
async fn response_does_not_leak_absolute_root_path_on_error() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/nonexistent"), &state).await;
    let body = body_bytes(resp).await;
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
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &state).await;
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
}

#[tokio::test]
async fn etag_header_present() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &state).await;
    let etag = resp.headers().get("etag").unwrap().to_str().unwrap();
    assert!(etag.starts_with("W/\""));
    assert!(etag.ends_with('"'));
}

#[tokio::test]
async fn last_modified_header_present() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &state).await;
    assert!(resp.headers().get("last-modified").is_some());
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/"), &state).await;
    let body = body_bytes(resp).await;
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/"), &state).await;
    let body = body_bytes(resp).await;
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/"), &state).await;
    assert_eq!(
        resp.headers().get("content-security-policy").unwrap(),
        "default-src 'none'; base-uri 'none'; form-action 'none'"
    );
    assert_eq!(
        resp.headers().get("referrer-policy").unwrap(),
        "no-referrer"
    );
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/"), &state).await;
    let body = body_bytes(resp).await;
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(head("/"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );

    let body = body_bytes(resp).await;
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/.env"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "SECRET");
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/link.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "real content");
}

#[tokio::test]
async fn get_root_without_index_returns_403() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn percent_encoded_path_serves_correct_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file with spaces.txt"), "spacey").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file%20with%20spaces.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "spacey");
}

#[tokio::test]
async fn subdir_file_served_correctly() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("a")).unwrap();
    fs::create_dir(tmp.path().join("a").join("b")).unwrap();
    fs::write(tmp.path().join("a").join("b").join("c.txt"), "nested").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/a/b/c.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "nested");
}

#[tokio::test]
async fn method_not_allowed_for_delete() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(method(Method::DELETE, "/file"), &state).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(resp.headers().get("allow").unwrap(), "GET, HEAD");
}

#[tokio::test]
async fn method_not_allowed_for_patch() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(method(Method::PATCH, "/file"), &state).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_missing() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let get_resp = handle_request(get("/nope"), &state).await;
    let head_resp = handle_request(head("/nope"), &state).await;
    assert_eq!(get_resp.status(), head_resp.status());
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_dotfile() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".hidden"), "secret").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let get_resp = handle_request(get("/.hidden"), &state).await;
    let head_resp = handle_request(head("/.hidden"), &state).await;
    assert_eq!(get_resp.status(), head_resp.status());
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_directory_without_index() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("emptydir")).unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let get_resp = handle_request(get("/emptydir"), &state).await;
    let head_resp = handle_request(head("/emptydir"), &state).await;
    assert_eq!(get_resp.status(), head_resp.status());
}

#[tokio::test]
async fn dotfile_denied_in_subdir() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("sub")).unwrap();
    fs::write(tmp.path().join("sub").join(".gitignore"), "*.o").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/sub/.gitignore"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/"), &state).await;
    let body = body_bytes(resp).await;
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("visible.txt"));
    assert!(!body_str.contains(".hidden"));
}

#[tokio::test]
async fn large_file_returns_correct_content_length() {
    let tmp = TempDir::new().unwrap();
    let content = "x".repeat(100_000);
    fs::write(tmp.path().join("big.txt"), &content).unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/big.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-length").unwrap(), "100000");

    let body = body_bytes(resp).await;
    assert_eq!(body.len(), 100_000);
}

#[cfg(unix)]
#[tokio::test]
async fn intermediate_symlink_denied_when_symlinks_denied() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("real_dir")).unwrap();
    fs::write(tmp.path().join("real_dir").join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir")).unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/link_dir/file.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp, policy);

    let resp = handle_request(get("/link_dir/file.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp).await;
    assert_eq!(body, "content");
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
    let state = make_state(&tmp_root, policy);

    let resp = handle_request(get("/out/file.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp_root, policy);

    let resp = handle_request(get("/escape.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[cfg(unix)]
#[tokio::test]
async fn nested_intermediate_symlink_denied() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("a")).unwrap();
    fs::create_dir(tmp.path().join("b")).unwrap();
    fs::write(tmp.path().join("b").join("file.txt"), "content").unwrap();
    std::os::unix::fs::symlink(tmp.path().join("b"), tmp.path().join("a").join("link_b")).unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/a/link_b/file.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_put_delete_patch_all_405() {
    let tmp = TempDir::new().unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    for m in [Method::PUT, Method::DELETE, Method::PATCH] {
        let resp = handle_request(method(m.clone(), "/file"), &state).await;
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{} should return 405",
            m
        );
    }
}

#[tokio::test]
async fn head_does_not_consume_file_stream_permit() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let max = state.config().limits.max_file_streams;
    let mut permits = Vec::with_capacity(max);
    for _ in 0..max {
        permits.push(
            state
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }

    let resp = handle_request(head("/file.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    drop(permits);
}

#[tokio::test]
async fn file_stream_permit_held_until_body_drop() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let max = state.config().limits.max_file_streams;
    let mut permits = Vec::with_capacity(max);
    for _ in 0..max - 1 {
        permits.push(
            state
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }

    let resp = handle_request(get("/file.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert!(
        state
            .file_stream_semaphore()
            .clone()
            .try_acquire_owned()
            .is_err(),
        "permit should be held while body exists"
    );

    drop(resp);

    assert!(
        state
            .file_stream_semaphore()
            .clone()
            .try_acquire_owned()
            .is_ok(),
        "permit should be released after body drop"
    );

    drop(permits);
}

#[tokio::test]
async fn double_encoded_dotdot_is_rejected() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/%252e%252e/hello.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn double_encoded_slash_is_treated_as_literal() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/%252f%252e%252e/hello.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn single_encoded_dotdot_is_rejected() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/%2e%2e/hello.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn encoded_dotfile_denied() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".env"), "secret").unwrap();
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/%2eenv"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp_root, policy);

    let resp = handle_request(get("/escape.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    let state = make_state(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/subdir"), &state).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_symlink_swap_stress() {
    use std::sync::atomic::{AtomicBool, Ordering};
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

    let state = Arc::new(make_state(&tmp_root, StaticPolicy::safe_default()));

    let outside_secret = tmp_outside.path().join("secret.txt");
    let link_path = tmp_root.path().join("link.txt");
    let safe_target = tmp_root.path().join("safe.txt");
    let leaked = Arc::new(AtomicBool::new(false));

    const ITERS: usize = 100;
    const THREADS: usize = 4;

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let state = Arc::clone(&state);
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

                    let resp = rt.block_on(handle_request(get("/link.txt"), &state));
                    let status = resp.status();

                    let body = rt.block_on(body_bytes(resp));
                    let body_str = String::from_utf8_lossy(&body);

                    if body_str.contains("LEAKED") {
                        leaked.store(true, Ordering::SeqCst);
                    }

                    assert!(
                        status == StatusCode::FORBIDDEN || status == StatusCode::OK,
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
