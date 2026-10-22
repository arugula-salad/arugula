//! Keys at E2B's egress proxy, live (M185): a key in a box's egress rules
//! (a project's own, held by the proxy) or in E2B's secrets broker (the
//! platform's) is set on the box's outbound HTTPS requests, over the
//! placeholder the box sent, and is never in the box's environment, its
//! processes' environments or its disk. Ignored unless asked for, since it
//! makes a real box and a real secret:
//!
//! ```sh
//! ARUGULA_TEST_E2B_URL=https://api.e2b.dev ARUGULA_TEST_E2B_KEY=... \
//!   cargo test -p arugula-platform --lib compute::egress_tests -- --ignored --nocapture
//! ```
//!
//! No model is called and no real key is used: the keys are random, and
//! the rules set them on requests to an echo service (`httpbin.org`),
//! whose answer the box hashes before printing, so a key never lands in
//! the box. The needle the box searches for is written as two halves the
//! shell joins, so no command line carries it whole (and the search runs
//! as root, not through sudo, which would log it). The box runs commands
//! through E2B's envd (as its SDKs do), which only this test does: the
//! platform never execs in a box. The box and the secret are deleted at
//! the end, whatever happened.
//!
//! What it checks:
//! - a key in the rules is set on both headers (`x-api-key`, and
//!   `authorization: Bearer …`), for curl and for Node (with the proxy's
//!   CA);
//! - the box's env and `/proc/*/environ` don't have it, and nor does its
//!   disk;
//! - new rules on a running box take effect (a rotated key), and clearing
//!   them sends the placeholder as the box wrote it;
//! - a rule naming a broker secret is filled from it, and a deleted secret
//!   fills nothing (E2B fails open, never sending the reference on).
//!
//! It also says, without failing, whether E2B's API answers a box's rules
//! back to whoever holds the platform's E2B key.

use std::{collections::BTreeMap, panic::AssertUnwindSafe, time::Duration};

use base64::Engine as _;
use futures_util::FutureExt as _;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::*;

fn sha(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))[..16].to_owned()
}

fn random_hex() -> String {
    hex::encode(arugula_e2e::random::<10>())
}

/// A box's envd, reached as E2B's SDKs reach it.
struct Envd {
    http: reqwest::Client,
    base: String,
    id: String,
    token: Option<String>,
}

impl Envd {
    /// Connect to a running box for its envd's address and token.
    async fn connect(api: &str, key: &str, id: &str) -> Envd {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(300)).build().unwrap();
        let r: Value = http
            .post(format!("{api}/v2/sandboxes/{id}/connect"))
            .header("X-API-Key", key)
            .json(&json!({"timeout": 300}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let domain = r["domain"].as_str().filter(|d| !d.is_empty()).unwrap_or("e2b.app").to_owned();
        Envd {
            http,
            base: format!("https://49983-{id}.{domain}"),
            id: id.into(),
            token: r["envdAccessToken"].as_str().map(str::to_owned),
        }
    }

    /// Run `cmd` with `bash -l -c` as `user`: its stdout and stderr.
    async fn run(&self, user: &str, cmd: &str) -> String {
        let msg = serde_json::to_vec(&json!({"process": {"cmd": "/bin/bash", "args": ["-l", "-c", cmd]}})).unwrap();
        let mut body = vec![0u8];
        body.extend((msg.len() as u32).to_be_bytes());
        body.extend(msg);
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("{user}:"));
        let mut req = self
            .http
            .post(format!("{}/process.Process/Start", self.base))
            .header("Content-Type", "application/connect+json")
            .header("Connect-Protocol-Version", "1")
            .header("Authorization", format!("Basic {auth}"))
            .header("E2b-Sandbox-Id", &self.id)
            .header("E2b-Sandbox-Port", "49983")
            .body(body);
        if let Some(t) = &self.token {
            req = req.header("X-Access-Token", t);
        }
        let bytes = req.send().await.unwrap().bytes().await.unwrap();
        // Connect's envelopes: a flag byte, a length, a JSON message.
        let (mut out, mut at) = (String::new(), 0);
        while at + 5 <= bytes.len() {
            let len = u32::from_be_bytes(bytes[at + 1..at + 5].try_into().unwrap()) as usize;
            let m: Value = serde_json::from_slice(&bytes[at + 5..at + 5 + len]).unwrap_or_default();
            at += 5 + len;
            for stream in ["stdout", "stderr"] {
                if let Some(b) = m["event"]["data"][stream].as_str() {
                    let b = base64::engine::general_purpose::STANDARD.decode(b).unwrap_or_default();
                    out.push_str(&String::from_utf8_lossy(&b));
                }
            }
            if let Some(e) = m.get("error") {
                out.push_str(&format!("[envd error {e}]"));
            }
        }
        out
    }
}

/// What httpbin saw, hashed in the box: `<x-api-key> <authorization>`,
/// each its hash, `ABSENT`, or the placeholder as sent.
const ECHO: &str = r#"curl -s https://httpbin.org/headers -H "x-api-key: $ANTHROPIC_API_KEY" -H "authorization: Bearer $ANTHROPIC_API_KEY" | python3 -c '
import sys, json, hashlib
h = json.load(sys.stdin)["headers"]
def show(v):
    if v is None: return "ABSENT"
    if "arugula-brokered" in v: return "PLACEHOLDER"
    return hashlib.sha256(v.encode()).hexdigest()[:16]
print(show(h.get("X-Api-Key")), show(h.get("Authorization")))'"#;

const NODE: &str = r#"NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/e2b-ca.crt node -e '
fetch("https://httpbin.org/headers", {headers: {"x-api-key": process.env.ANTHROPIC_API_KEY}})
  .then(r => r.json())
  .then(j => console.log(require("crypto").createHash("sha256").update(j.headers["X-Api-Key"] || "").digest("hex").slice(0, 16)))
  .catch(e => console.log("node failed", (e.cause || e).code))'"#;

/// E2B's secrets API, for the broker's half of the test: the platform
/// never keeps a secret there itself (the operator does, for its own
/// keys).
async fn secret(api: &str, key: &str, method: reqwest::Method, name: &str, value: Option<&str>) -> u16 {
    let http = reqwest::Client::new();
    let req = match value {
        Some(v) => http.request(method, format!("{api}/secrets")).json(&json!({"name": name, "value": v})),
        None => http.request(method, format!("{api}/secrets/{name}")),
    };
    req.header("X-API-Key", key).send().await.unwrap().status().as_u16()
}

#[tokio::test]
#[ignore = "makes a real box and secret: set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY"]
async fn keys_at_egress_never_enter_the_box() {
    let (Ok(url), Ok(key)) = (std::env::var("ARUGULA_TEST_E2B_URL"), std::env::var("ARUGULA_TEST_E2B_KEY")) else {
        panic!("set ARUGULA_TEST_E2B_URL and ARUGULA_TEST_E2B_KEY");
    };
    let e2b = E2b::new(&url, key.clone()).unwrap();
    let run_id = format!("m185-{}", random_hex());
    let tag: BTreeMap<String, String> =
        [("arugula_test".into(), "1".into()), ("arugula_run".into(), run_id.clone())].into();
    let name = format!("arugula-test-{run_id}");
    println!("run {run_id} against {url}");

    let body = AssertUnwindSafe(checks(&e2b, &url, &key, &tag, &name)).catch_unwind().await;

    // Clean up whatever happened.
    if let Ok(found) = e2b.list(&tag).await {
        for b in found {
            println!("cleanup delete {}: {:?}", b.id, e2b.delete(&b.id).await);
        }
    }
    println!("cleanup secret: {}", secret(&url, &key, reqwest::Method::DELETE, &name, None).await);
    let after = e2b.list(&tag).await.unwrap();
    assert!(after.is_empty(), "boxes left: {after:?}");
    match body {
        Ok(failed) => assert!(failed.is_empty(), "failed: {failed:?}"),
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// Both headers set to `v` on requests to the echo host.
fn echo_rules(v: &str) -> Rules {
    [(
        "httpbin.org".to_owned(),
        [("x-api-key".to_owned(), v.to_owned()), ("authorization".to_owned(), format!("Bearer {v}"))].into(),
    )]
    .into()
}

/// Every check, printed: the ones that failed.
async fn checks(e2b: &E2b, url: &str, key: &str, tag: &BTreeMap<String, String>, name: &str) -> Vec<String> {
    let mut failed = vec![];
    let mut check = |what: &str, ok: bool, got: &str| {
        println!("{} {what}: {got}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            failed.push(what.to_owned());
        }
    };
    let (a, b) = (random_hex(), random_hex());
    let value = format!("m185test-{a}{b}");
    // The needle, joined by the box's shell: never whole on a command line.
    let needle = format!(r#""$(printf '%s%s%s' m185test- {a} {b})""#);
    let made = e2b
        .create(&Create {
            template: "base".into(),
            env: [("ANTHROPIC_API_KEY".into(), "arugula-brokered".into())].into(),
            metadata: tag.clone(),
            timeout: Duration::from_secs(300),
            rules: echo_rules(&value),
        })
        .await
        .unwrap();
    let envd = Envd::connect(url, key, &made.id).await;
    let hashed = |v: &str| format!("{} {}", sha(v), sha(&format!("Bearer {v}")));

    let got = envd.run("user", ECHO).await;
    check(
        "a key in the rules is set on both headers over the placeholder (curl)",
        got.trim() == hashed(&value),
        got.trim(),
    );
    let got = envd.run("user", NODE).await;
    check("and for Node, with the proxy's CA", got.trim() == sha(&value), got.trim());
    let got = envd
        .run("user", &format!("env | grep -c {needle}; cat /proc/[0-9]*/environ 2>/dev/null | grep -caF {needle}"))
        .await;
    check(
        "the key isn't in the box's env or any process's",
        got.split_whitespace().collect::<Vec<_>>() == ["0", "0"],
        got.trim(),
    );
    let got = envd
        .run("root", &format!("grep -rlaF {needle} / --exclude-dir=proc --exclude-dir=sys --exclude-dir=dev 2>/dev/null; echo searched"))
        .await;
    check("nor on its disk", got.trim() == "searched", got.trim());

    let detail = reqwest::Client::new()
        .get(format!("{url}/sandboxes/{}", made.id))
        .header("X-API-Key", key)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap_or_default();
    println!("info E2B's API answers the box's rules back to the E2B key's holder: {}", detail.contains(&value));

    let next = format!("m185test-{}", random_hex());
    e2b.set_rules(&made.id, &echo_rules(&next)).await.unwrap();
    let got = envd.run("user", ECHO).await;
    check("a rotated key in new rules is used on the next request", got.trim() == hashed(&next), got.trim());
    e2b.set_rules(&made.id, &Rules::new()).await.unwrap();
    let got = envd.run("user", ECHO).await;
    check("rules cleared: the placeholder goes as sent", got.trim() == "PLACEHOLDER PLACEHOLDER", got.trim());

    // The platform's own key: a broker secret the rules name.
    let platform = format!("m185test-{}", random_hex());
    assert_eq!(secret(url, key, reqwest::Method::POST, name, Some(&platform)).await, 201);
    e2b.set_rules(&made.id, &echo_rules(&fill(name))).await.unwrap();
    let got = envd.run("user", ECHO).await;
    check("a broker secret the rules name is filled in", got.trim() == hashed(&platform), got.trim());
    secret(url, key, reqwest::Method::DELETE, name, None).await;
    let got = envd.run("user", ECHO).await;
    check("a deleted secret fills nothing", got.trim() == "ABSENT ABSENT", got.trim());
    failed
}
