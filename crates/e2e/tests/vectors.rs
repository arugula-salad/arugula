//! The checked-in certificate vectors (`crates/e2e/fixtures/certs.json`,
//! from `interop fixtures`, #200) still evaluate as they did when they were
//! made: the same trusted devices and the same join code. Another
//! implementation checks itself against the same file with no Rust
//! toolchain (`web/e2e-interop.ts` does).

use arugula_e2e::{Cert, Revocation, Trust, cert::join_code};
use serde_json::Value;

#[test]
fn certificate_vectors_still_hold() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/certs.json")).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let trust: Trust = serde_json::from_value(v["trust"].clone()).unwrap();
    let certs: Vec<Cert> = serde_json::from_value(v["certs"].clone()).unwrap();
    let revocations: Vec<Revocation> = serde_json::from_value(v["revocations"].clone()).unwrap();
    let mut trusted: Vec<String> = trust.evaluate(&certs, &revocations).devices.into_keys().collect();
    trusted.sort();
    let want: Vec<String> = serde_json::from_value(v["trusted"].clone()).unwrap();
    assert_eq!(trusted, want);
    let daemon: Cert = serde_json::from_value(v["daemon"].clone()).unwrap();
    assert_eq!(join_code(&daemon), v["joinCode"]);
}
