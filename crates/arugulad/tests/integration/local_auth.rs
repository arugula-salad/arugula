//! Loopback callers show the local token: programs as a bearer, browsers
//! the page key a sign-in link gives the page, in a header. Never a cookie.
//! The Unix socket needs none.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
};

use arugula_testkit::{Daemon, arugulad};
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

fn start() -> Daemon {
    arugulad!("lauth").no_wisp().no_tailscale().env_remove("ARUGULA_LOCAL_TOKEN_FILE").start()
}

/// One request over TCP with these headers: the response's head
/// (lowercase) and body.
fn tcp(d: &Daemon, method: &str, path: &str, headers: &[(&str, &str)]) -> (u16, String, String) {
    d.tcp(method, path, headers, None)
}

/// The same over the Unix socket, with no credential.
fn socket(d: &Daemon, path: &str) -> (u16, String, String) {
    let mut s = UnixStream::connect(d.sock()).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    response(s)
}

fn response(mut s: impl Read) -> (u16, String, String) {
    let mut res = Vec::new();
    s.read_to_end(&mut res).unwrap();
    let res = String::from_utf8_lossy(&res).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap_or((&res, ""));
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.to_ascii_lowercase(), body.to_owned())
}

#[test]
fn loopback_programs_and_browsers_show_the_local_token() {
    let d = start();
    let token = d.token();
    assert!(token.starts_with("ilt_"), "{token}");
    let mode = std::fs::metadata(d.state.join("local-token")).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "the token file is the owner's alone");

    // Loopback alone is not enough (the refusal tells the page to sign
    // in); the app's own files need nothing.
    let (status, head, _) = tcp(&d, "GET", "/api/panes", &[]);
    assert_eq!(status, 401);
    assert!(head.contains("\r\nx-arugula-sign-in: /auth\r\n"), "{head}");
    let (status, _, page) = tcp(&d, "GET", "/", &[("accept", "text/html")]);
    assert!([200, 404].contains(&status), "the page (or 'not built'), not a refusal: {status} {page}");
    // A program: the bearer.
    let bearer = format!("Bearer {token}");
    assert_eq!(tcp(&d, "GET", "/api/panes", &[("authorization", &bearer)]).0, 200);
    assert_eq!(tcp(&d, "GET", "/api/panes", &[("authorization", "Bearer ilt_wrong")]).0, 401);
    // A browser: the page key, in its header.
    let key = page_key(&d);
    assert_eq!(tcp(&d, "GET", "/api/panes", &[("x-arugula-key", &key)]).0, 200);
    assert_eq!(tcp(&d, "GET", "/api/panes", &[("x-arugula-key", "ilp_wrong")]).0, 401);
    assert_eq!(tcp(&d, "GET", "/api/panes", &[("authorization", &format!("Bearer {key}"))]).0, 401);
    // Never a cookie (a browser sends one to every port on the host): not
    // the token, which older sign-in links set, nor the key.
    for v in [&token, &key] {
        let cookie = format!("arugula_{}={v}", d.port);
        let (status, _, page) = tcp(&d, "GET", "/api/panes", &[("cookie", &cookie), ("sec-fetch-site", "same-origin")]);
        assert_eq!(status, 401, "a cookie with {v:.4}…");
        assert!(page.contains("arugula web"), "it says how to sign in: {page}");
    }
    // The socket is the owner's already.
    assert_eq!(socket(&d, "/api/panes").0, 200);
}

/// The page key the sign-in link's page stores.
fn page_key(d: &Daemon) -> String {
    let (status, _, page) = tcp(d, "GET", &format!("/auth?token={}", d.token()), &[]);
    assert_eq!(status, 200, "{page}");
    let at = page.find("\"ilp_").expect(&page) + 1;
    page[at..at + 68].to_owned()
}

#[test]
fn a_signin_link_gives_the_page_its_key_and_goes_to_the_page() {
    let d = start();
    let token = d.token();
    assert_eq!(tcp(&d, "GET", "/auth?token=ilt_wrong", &[]).0, 401);
    let (status, head, page) = tcp(&d, "GET", &format!("/auth?token={token}&next=/x"), &[]);
    assert_eq!(status, 200, "{head}");
    assert!(page.contains(r#"location.replace("/x")"#), "{page}");
    assert!(!page.contains(&token), "the page has the key, not the token: {page}");
    let key = page_key(&d);
    assert!(page.contains(&format!(r#"localStorage.setItem("arugula.key","{key}")"#)), "{page}");
    // It sets no cookie, and removes an older link's.
    let clear = format!("set-cookie: arugula_{}=; path=/; httponly; samesite=lax; max-age=0", d.port);
    assert!(head.contains(&clear), "{head}");
    assert!(head.contains("cache-control: no-store"), "{head}");
    // Only a path of ours.
    for next in ["//evil.example/", "https://evil.example/", "/%5Cevil.example"] {
        let (status, _, page) = tcp(&d, "GET", &format!("/auth?token={token}&next={next}"), &[]);
        assert_eq!(status, 200);
        assert!(page.contains(r#"location.replace("/")"#), "{next}: {page}");
    }
    // Without a token: a browser with an older link's cookie is signed in
    // from it, once; one without is told how to sign in.
    let old = format!("arugula_{}={token}", d.port);
    let (status, head, page) = tcp(&d, "GET", "/auth?next=/y", &[("cookie", &old)]);
    assert_eq!(status, 200, "{head}");
    assert!(page.contains(&key) && page.contains(r#"location.replace("/y")"#), "{page}");
    assert!(head.contains(&clear), "{head}");
    let (status, _, page) = tcp(&d, "GET", "/auth?next=/y", &[("cookie", &format!("arugula_{}={key}", d.port))]);
    assert_eq!(status, 401);
    assert!(page.contains("arugula web"), "{page}");
    assert_eq!(tcp(&d, "GET", "/auth", &[]).0, 401);

    // `arugula web`'s link: over the socket only.
    let (status, _, body) = socket(&d, "/api/signin-link");
    assert_eq!(status, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["url"], format!("http://127.0.0.1:{}/auth?token={token}", d.port));
    let bearer = format!("Bearer {token}");
    assert_eq!(tcp(&d, "GET", "/api/signin-link", &[("authorization", &bearer)]).0, 404);
}

#[tokio::test]
async fn the_socket_and_mcp_over_loopback_need_it_too() {
    let d = start();
    let token = d.token();
    let ws = |auth: Option<String>| {
        let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
        if let Some(a) = auth {
            req.headers_mut().insert("authorization", a.parse().unwrap());
        }
        connect_async(req)
    };
    match ws(None).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => assert_eq!(r.status(), 401),
        other => panic!("a WebSocket without the token: {:?}", other.map(|_| ())),
    }
    assert!(ws(Some(format!("Bearer {token}"))).await.is_ok());
    // A browser's: the page key in its URL (it can't send a header).
    let key = page_key(&d);
    let url = |k: &str| connect_async(format!("ws://127.0.0.1:{}/ws?arugula_key={k}", d.port));
    assert!(url(&key).await.is_ok());
    match url("ilp_wrong").await {
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => assert_eq!(r.status(), 401),
        other => panic!("a WebSocket with a wrong key: {:?}", other.map(|_| ())),
    }
    let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
    req.headers_mut().insert("cookie", format!("arugula_{}={token}", d.port).parse().unwrap());
    assert!(connect_async(req).await.is_err(), "not with a cookie");

    // MCP: refused without it; the local token works as its bearer.
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"x","version":"1"}}}"#;
    let mcp = |auth: Option<String>| {
        let mut r = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{}/mcp", d.port))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .body(body);
        if let Some(a) = auth {
            r = r.header("Authorization", a);
        }
        r.send()
    };
    assert_eq!(mcp(None).await.unwrap().status(), 401);
    assert!([401, 403].contains(&mcp(Some("Bearer ilt_wrong".into())).await.unwrap().status().as_u16()));
    let ok = mcp(Some(format!("Bearer {token}"))).await.unwrap().status().as_u16();
    assert!(ok < 300, "the local token as MCP's bearer: {ok}");
}
