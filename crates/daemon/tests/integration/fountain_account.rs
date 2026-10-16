//! Retired direct accounts never reconnect or mutate a user's old credentials.
#![cfg(unix)]

use crate::agentd::*;
use serde_json::json;
use std::{net::TcpListener, time::Duration};

#[test]
fn saved_credentials_and_stream_flags_never_contact_managoat() {
    // A TCP listener detects even a connection that never becomes an HTTP request.
    let provider = TcpListener::bind("127.0.0.1:0").unwrap();
    provider.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", provider.local_addr().unwrap());
    let home = Scratch::new("retired-managoat-home");
    let home_name = home.display().to_string();
    let saved = json!({"base_url":origin,"key":"retired-test-key","key_id":"key-1","at_ms":1}).to_string();
    let mut d = Daemon::child_in(
        &["--managoat-account-stream"],
        &[
            ("HOME", &home_name),
            ("MANAGOAT_API_KEY", "retired-env-key"),
            ("MANAGOAT_BASE_URL", &origin),
            ("FOUNTAIN_API_KEY", "retired-env-key"),
            ("FOUNTAIN_BASE_URL", &origin),
            ("ARUGULA_FOUNTAIN_STREAM_RETRY_MS", "50"),
        ],
        |state| {
            arugula_testkit::labs(state, true);
            std::fs::create_dir_all(state.join("managoat")).unwrap();
            std::fs::write(state.join("managoat/login.json"), &saved).unwrap();
        },
    );
    for restart in [false, true] {
        if restart {
            d.stop();
            d.start();
        }
        for method in ["GET", "POST", "DELETE"] {
            let body = (method == "POST").then(|| json!({"base_url":origin}));
            let (status, response) = d.raw(method, "/api/fountain/login", body);
            assert_eq!(status, 410, "{method}: {response}");
            assert!(response.contains("retired") && response.contains("Cloud agents"), "{response}");
            assert!(!response.contains("retired-test-key") && !response.contains("retired-env-key"));
        }
        for agent in ["managoat", "fountain"] {
            let (status, response) = d.raw(
                "POST",
                "/api/blocks",
                Some(json!({"type":"agent","config":{"agent":agent,"managoat_agent":"legacy"}})),
            );
            assert_eq!(status, 400, "{response}");
            assert!(response.contains("retired"), "{response}");
        }
        for kind in ["managoat", "fountain"] {
            let (status, response) = d.raw("POST", "/api/blocks", Some(json!({"type":kind,"config":{}})));
            assert_eq!(status, 400, "{response}");
            assert!(response.contains("retired"), "{response}");
        }
        let (status, response) = d.raw("GET", "/api/fountain/agents", None);
        assert_eq!(status, 400, "{response}");
        assert!(response.contains("retired"), "{response}");
        for operation in ["preflight", "copy"] {
            let (status, response) = d.raw(
                "POST",
                &format!("/api/a2a/fountain/{operation}"),
                Some(json!({"agent":"legacy","var":"RECIPE_TOKEN","vault":"old-vault"})),
            );
            assert_eq!(status, 400, "{response}");
            assert!(response.contains("retired"), "{response}");
        }
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(
            provider.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "retired credentials must never connect to the provider"
        );
        assert_eq!(
            std::fs::read_to_string(d.state.join("managoat/login.json")).unwrap(),
            saved,
            "the old credential remains available for an explicit import"
        );
    }
}
