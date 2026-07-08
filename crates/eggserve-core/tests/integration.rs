use std::fs;

use eggserve_core::config::ServeConfig;
use eggserve_core::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy, SymlinkPolicy};
use eggserve_core::service::handle_request;
use http_body_util::BodyExt;
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use tempfile::TempDir;

fn make_config(tmp: &TempDir, policy: StaticPolicy) -> ServeConfig {
    ServeConfig {
        root: tmp.path().to_path_buf(),
        static_policy: policy,
        ..ServeConfig::default()
    }
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

async fn body_bytes(resp: hyper::Response<eggserve_core::response::BoxBodyInner>) -> Bytes {
    resp.into_body().collect().await.unwrap().to_bytes()
}

#[tokio::test]
async fn get_existing_file_returns_200_with_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/hello.txt"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "hello world");
}

#[tokio::test]
async fn head_existing_file_returns_200_empty_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(head("/hello.txt"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body.len(), 0);
}

#[tokio::test]
async fn get_missing_file_returns_404() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/nonexistent.txt"), &config).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let body = body_bytes(resp).await;
    assert_eq!(body, "404 Not Found\n");
}

#[tokio::test]
async fn get_denied_dotfile_returns_403() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".env"), "SECRET_KEY=abc").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/.env"), &config).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let body = body_bytes(resp).await;
    assert_eq!(body, "403 Forbidden\n");
}

#[tokio::test]
async fn get_symlink_returns_403_under_safe_default() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("real.txt"), "real content").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/link.txt"), &config).await;
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
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/subdir"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "<html>index</html>");
}

#[tokio::test]
async fn get_directory_without_index_returns_403_when_listing_disabled() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/subdir"), &config).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_unsupported_method_returns_405() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(method(Method::POST, "/anything"), &config).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(resp.headers().get("allow").unwrap(), "GET, HEAD");
}

#[tokio::test]
async fn content_length_matches_file_length() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "hello").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-length").unwrap(), "5");
}

#[tokio::test]
async fn content_type_defaults_to_octet_stream_for_unknown_extension() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.xyz"), "data").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.xyz"), &config).await;
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
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.html"), &config).await;
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );

    let resp = handle_request(get("/style.css"), &config).await;
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/css; charset=utf-8"
    );

    let resp = handle_request(get("/script.js"), &config).await;
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/javascript; charset=utf-8"
    );
}

#[tokio::test]
async fn response_does_not_leak_absolute_root_path_on_error() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/nonexistent"), &config).await;
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
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &config).await;
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
}

#[tokio::test]
async fn etag_header_present() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &config).await;
    let etag = resp.headers().get("etag").unwrap().to_str().unwrap();
    assert!(etag.starts_with("W/\""));
    assert!(etag.ends_with('"'));
}

#[tokio::test]
async fn last_modified_header_present() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file.txt"), "data").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file.txt"), &config).await;
    assert!(resp.headers().get("last-modified").is_some());
}

#[tokio::test]
async fn directory_listing_enabled_shows_entries() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(tmp.path().join("a.txt"), "a").unwrap();
    fs::write(tmp.path().join("b.txt"), "b").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let config = make_config(&tmp, policy);

    let resp = handle_request(get("/"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("a.txt"));
    assert!(body_str.contains("b.txt"));
    assert!(body_str.contains("subdir"));
}

#[tokio::test]
async fn directory_listing_escapes_html() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file with 'quotes' & ampersand"), "xss").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let config = make_config(&tmp, policy);

    let resp = handle_request(get("/"), &config).await;
    let body = body_bytes(resp).await;
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("file with &#x27;quotes&#x27; &amp; ampersand"));
}

#[tokio::test]
async fn directory_listing_has_security_headers() {
    let tmp = TempDir::new().unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let config = make_config(&tmp, policy);

    let resp = handle_request(get("/"), &config).await;
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
    let config = make_config(&tmp, policy);

    let resp = handle_request(get("/"), &config).await;
    let body = body_bytes(resp).await;
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        !body_str.contains(&tmp.path().to_string_lossy().to_string()),
        "listing should not contain absolute filesystem path"
    );
}

#[tokio::test]
async fn directory_listing_head_has_no_body() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.txt"), "a").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        ..StaticPolicy::safe_default()
    };
    let config = make_config(&tmp, policy);

    let resp = handle_request(head("/"), &config).await;
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
    let config = make_config(&tmp, policy);

    let resp = handle_request(get("/.env"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "SECRET");
}

#[tokio::test]
async fn symlink_followed_when_policy_permits() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("real.txt"), "real content").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt")).unwrap();
    let policy = StaticPolicy {
        symlinks: SymlinkPolicy::Follow,
        ..StaticPolicy::safe_default()
    };
    let config = make_config(&tmp, policy);

    let resp = handle_request(get("/link.txt"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "real content");
}

#[tokio::test]
async fn get_root_without_index_returns_403() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/"), &config).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn percent_encoded_path_serves_correct_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file with spaces.txt"), "spacey").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/file%20with%20spaces.txt"), &config).await;
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
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/a/b/c.txt"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_bytes(resp).await;
    assert_eq!(body, "nested");
}

#[tokio::test]
async fn method_not_allowed_for_delete() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(method(Method::DELETE, "/file"), &config).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(resp.headers().get("allow").unwrap(), "GET, HEAD");
}

#[tokio::test]
async fn method_not_allowed_for_patch() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(method(Method::PATCH, "/file"), &config).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_missing() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let get_resp = handle_request(get("/nope"), &config).await;
    let head_resp = handle_request(head("/nope"), &config).await;
    assert_eq!(get_resp.status(), head_resp.status());
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_dotfile() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".hidden"), "secret").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let get_resp = handle_request(get("/.hidden"), &config).await;
    let head_resp = handle_request(head("/.hidden"), &config).await;
    assert_eq!(get_resp.status(), head_resp.status());
}

#[tokio::test]
async fn head_returns_same_status_as_get_for_directory_without_index() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("emptydir")).unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let get_resp = handle_request(get("/emptydir"), &config).await;
    let head_resp = handle_request(head("/emptydir"), &config).await;
    assert_eq!(get_resp.status(), head_resp.status());
}

#[tokio::test]
async fn dotfile_denied_in_subdir() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("sub")).unwrap();
    fs::write(tmp.path().join("sub").join(".gitignore"), "*.o").unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/sub/.gitignore"), &config).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn directory_listing_denies_dotfile_entries() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(".hidden"), "secret").unwrap();
    fs::write(tmp.path().join("visible.txt"), "public").unwrap();
    let policy = StaticPolicy {
        directory_listing: DirectoryListingPolicy::Enabled,
        dotfiles: DotfilePolicy::Denied,
        ..StaticPolicy::safe_default()
    };
    let config = make_config(&tmp, policy);

    let resp = handle_request(get("/"), &config).await;
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
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/big.txt"), &config).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-length").unwrap(), "100000");

    let body = body_bytes(resp).await;
    assert_eq!(body.len(), 100_000);
}

#[tokio::test]
async fn symlink_index_denied_when_symlinks_denied() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(tmp.path().join("real_index.html"), "real").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        tmp.path().join("real_index.html"),
        tmp.path().join("subdir").join("index.html"),
    )
    .unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    let resp = handle_request(get("/subdir"), &config).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_put_delete_patch_all_405() {
    let tmp = TempDir::new().unwrap();
    let config = make_config(&tmp, StaticPolicy::safe_default());

    for m in [Method::PUT, Method::DELETE, Method::PATCH] {
        let resp = handle_request(method(m.clone(), "/file"), &config).await;
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{} should return 405",
            m
        );
    }
}
