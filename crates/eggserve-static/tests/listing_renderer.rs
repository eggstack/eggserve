//! Plan 311: the public listing renderer is the single listing authority.
//!
//! These tests pin the exact bytes the renderer emits (the canonical listing
//! format every surface must serve), the byte-ceiling refusals, and the
//! shared default bounds. They also prove that the static service — the Rust
//! surface — emits exactly the bytes the public renderer produces.

use std::future::Future;
use std::task::{Context, Poll};

use eggserve_primitives::{
    ConnectionInfo, DirectoryListingPolicy, HeaderBlock, HttpVersion, Method, Request, RequestBody,
    RequestHead, RequestTarget, Response, ResponseBody, Scheme, StaticPolicy,
};
use eggserve_server::Service;
use eggserve_static::{
    render_directory_listing, ListingEntry, StaticService, DEFAULT_MAX_LISTING_ENTRIES,
    DEFAULT_MAX_LISTING_RESPONSE_BYTES,
};

const PREFIX: &str = "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Directory listing</title>\n</head>\n<body>\n<h1>Directory listing</h1>\n<ul>\n";
const SUFFIX: &str = "</ul>\n</body>\n</html>\n";

fn entry(name: &str, href: &str, is_dir: bool) -> ListingEntry {
    ListingEntry {
        name: name.to_owned(),
        href: href.to_owned(),
        is_dir,
    }
}

/// Poll a future to completion without an async runtime.
///
/// The static service resolves and renders without awaiting I/O, so a no-op
/// waker is enough to observe the response.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(std::task::Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn get_request(target: &str) -> Request {
    Request::new(
        RequestHead::new(
            Method::get(),
            RequestTarget::parse(target).expect("valid request target"),
            HttpVersion::Http11,
            HeaderBlock::new(),
        ),
        RequestBody::empty(),
        ConnectionInfo::without_socket_addrs(Scheme::Http, None),
    )
}

fn response_bytes(response: &Response) -> Vec<u8> {
    match response.body() {
        Some(ResponseBody::Bytes(bytes)) => bytes.clone(),
        other => panic!("expected an in-memory listing body, got {other:?}"),
    }
}

#[test]
fn public_renderer_emits_the_canonical_listing_bytes() {
    // One entry per escape branch (`&`, `<`, `>`, `"`, `'`, a control
    // character) plus a name whose raw bytes are not valid UTF-8.
    let entries = vec![
        entry("a&b.txt", "a%26b.txt", false),
        entry("<lt>.txt", "%3Clt%3E.txt", false),
        entry("apo'strophe.txt", "apo%27strophe.txt", false),
        entry("ctrl\u{1b}.txt", "ctrl%1B.txt", false),
        entry("quo\"te.txt", "quo%22te.txt", false),
        ListingEntry::from_raw_bytes(b"raw\xff.bin", false),
        entry("sub", "sub", true),
    ];
    let expected = concat!(
        "<!DOCTYPE html>\n",
        "<html>\n",
        "<head>\n",
        "<meta charset=\"utf-8\">\n",
        "<title>Directory listing</title>\n",
        "</head>\n",
        "<body>\n",
        "<h1>Directory listing</h1>\n",
        "<ul>\n",
        "<li><a href=\"a%26b.txt\">a&amp;b.txt</a></li>\n",
        "<li><a href=\"%3Clt%3E.txt\">&lt;lt&gt;.txt</a></li>\n",
        "<li><a href=\"apo%27strophe.txt\">apo&#x27;strophe.txt</a></li>\n",
        "<li><a href=\"ctrl%1B.txt\">ctrl&#x1B;.txt</a></li>\n",
        "<li><a href=\"quo%22te.txt\">quo&quot;te.txt</a></li>\n",
        "<li><a href=\"raw%FF.bin\">raw\u{FFFD}.bin</a></li>\n",
        "<li><a href=\"sub/\">sub/</a></li>\n",
        "</ul>\n",
        "</body>\n",
        "</html>\n",
    );
    let rendered = render_directory_listing(&entries, DEFAULT_MAX_LISTING_RESPONSE_BYTES)
        .expect("the sample listing fits the default byte ceiling");
    assert_eq!(rendered, expected.as_bytes());
}

#[test]
fn renderer_refuses_when_the_wrapper_does_not_fit() {
    let minimum = PREFIX.len() + SUFFIX.len();
    let empty = render_directory_listing(&[], minimum).expect("the empty wrapper fits exactly");
    assert_eq!(empty, format!("{PREFIX}{SUFFIX}").as_bytes());

    let error = render_directory_listing(&[], minimum - 1)
        .expect_err("a wrapper that cannot fit must be refused");
    assert!(
        error
            .message()
            .contains("directory listing exceeds configured bound"),
        "unexpected refusal message: {}",
        error.message()
    );
}

#[test]
fn renderer_refuses_when_a_link_would_exceed_the_ceiling() {
    let entries = vec![entry("a.txt", "a.txt", false)];
    let link = "<li><a href=\"a.txt\">a.txt</a></li>\n";
    let exact = PREFIX.len() + link.len() + SUFFIX.len();

    let rendered = render_directory_listing(&entries, exact).expect("the listing fits exactly");
    assert_eq!(rendered.len(), exact);

    let error = render_directory_listing(&entries, exact - 1)
        .expect_err("one byte over the ceiling must be refused");
    assert!(
        error
            .message()
            .contains("directory listing exceeds configured bound"),
        "unexpected refusal message: {}",
        error.message()
    );
}

#[test]
fn renderer_refuses_instead_of_truncating_a_long_listing() {
    // A listing that would blow the ceiling is refused whole: the renderer
    // never returns a truncated body.
    let entries: Vec<ListingEntry> = (0..64)
        .map(|index| entry(&format!("{index}.txt"), &format!("{index}.txt"), false))
        .collect();
    let full = render_directory_listing(&entries, DEFAULT_MAX_LISTING_RESPONSE_BYTES)
        .expect("the listing fits the default byte ceiling");
    let error = render_directory_listing(&entries, full.len() - 1)
        .expect_err("a listing one byte over the ceiling must be refused");
    assert!(
        error
            .message()
            .contains("directory listing exceeds configured bound"),
        "unexpected refusal message: {}",
        error.message()
    );
}

#[test]
fn default_listing_bounds_are_the_documented_values() {
    // The entry ceiling and the byte ceiling are shared with the
    // compatibility core and the Python bridge; the values are unchanged.
    assert_eq!(DEFAULT_MAX_LISTING_ENTRIES, 4096);
    assert_eq!(DEFAULT_MAX_LISTING_RESPONSE_BYTES, 1024 * 1024);
}

#[test]
fn static_service_emits_the_same_bytes_as_the_public_renderer() {
    let directory = tempfile::tempdir().expect("temporary directory");
    std::fs::write(directory.path().join("a.txt"), b"a").expect("write a.txt");
    std::fs::write(directory.path().join("b&c.txt"), b"b").expect("write b&c.txt");
    std::fs::create_dir(directory.path().join("sub")).expect("create sub directory");

    let mut policy = StaticPolicy::safe_default();
    policy.directory_listing = DirectoryListingPolicy::Enabled;
    let service = StaticService::builder(directory.path())
        .policy(policy)
        .build()
        .expect("static service");

    let response = block_on(service.call(get_request("/"))).expect("listing response");
    assert_eq!(response.head().status().as_u16(), 200);
    let body = response_bytes(&response);

    let expected = concat!(
        "<!DOCTYPE html>\n",
        "<html>\n",
        "<head>\n",
        "<meta charset=\"utf-8\">\n",
        "<title>Directory listing</title>\n",
        "</head>\n",
        "<body>\n",
        "<h1>Directory listing</h1>\n",
        "<ul>\n",
        "<li><a href=\"a.txt\">a.txt</a></li>\n",
        "<li><a href=\"b%26c.txt\">b&amp;c.txt</a></li>\n",
        "<li><a href=\"sub/\">sub/</a></li>\n",
        "</ul>\n",
        "</body>\n",
        "</html>\n",
    );
    assert_eq!(body, expected.as_bytes());
}

#[test]
fn static_service_refuses_an_oversized_listing() {
    let directory = tempfile::tempdir().expect("temporary directory");
    // One long-named file is enough to push the wrapper plus a single link
    // past a deliberately tiny byte ceiling.
    std::fs::write(directory.path().join("a.txt"), b"a").expect("write a.txt");

    let mut policy = StaticPolicy::safe_default();
    policy.directory_listing = DirectoryListingPolicy::Enabled;
    let service = StaticService::builder(directory.path())
        .policy(policy)
        .listing_limits(DEFAULT_MAX_LISTING_ENTRIES, PREFIX.len() + SUFFIX.len())
        .build()
        .expect("static service");

    let error = block_on(service.call(get_request("/"))).expect_err("oversized listing is refused");
    assert!(
        error
            .message()
            .contains("directory listing exceeds configured bound"),
        "unexpected refusal message: {}",
        error.message()
    );
}
