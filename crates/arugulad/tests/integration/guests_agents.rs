//! M14: an agent a guest starts beside the owner's pane, on a VM of the
//! guest's, reads what runs on its own machine. The owner's panes in the
//! same tab run here, as the owner: their files, output and history are
//! the owner's.

#![cfg(unix)]

use crate::agentd::*;
use hmac::{Hmac, KeyInit, Mac};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation},
    transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig},
};
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";

#[derive(Clone)]
struct C;

impl rmcp::ClientHandler for C {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(ClientCapabilities::default(), Implementation::new("guests-agents", "1"))
    }
}

/// The token the daemon gives agent block `id` (`mcp::Tokens`: an HMAC
/// of "arugula block {id}" under `mcp/key`).
fn block_token(d: &Daemon, id: u64) -> String {
    let key = std::fs::read(d.state.join("mcp/key")).unwrap();
    let mut m = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(&key).unwrap();
    m.update(format!("arugula block {id}").as_bytes());
    format!("ilb_{id}_{}", hex::encode(m.finalize().into_bytes()))
}

/// One tool call with `token`: its structured content, or its error.
fn call(rt: &tokio::runtime::Runtime, port: u16, token: &str, tool: &str, args: Value) -> Result<Value, String> {
    rt.block_on(async {
        let cfg =
            StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp")).auth_header(token);
        let s = C.serve(StreamableHttpClientTransport::from_config(cfg)).await.map_err(|e| e.to_string())?;
        let p =
            CallToolRequestParams::new(tool.to_owned()).with_arguments(args.as_object().cloned().unwrap_or_default());
        let r = s.call_tool(p).await.map_err(|e| e.to_string())?;
        let _ = s.cancel().await;
        if r.is_error == Some(true) {
            return Err(r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect());
        }
        Ok(r.structured_content.unwrap_or_default())
    })
}

#[test]
fn a_guests_agent_reads_its_own_machine_not_the_owners_panes() {
    let home = std::env::temp_dir().join(format!("ilg-guests-agents-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("notes.txt"), "OWNERS-NOTES\n").unwrap();
    // A sandbox provider that's set up but answers nothing: enough for the
    // guest's agent to get a VM of its own.
    let loam_key = home.join("loam-key");
    std::fs::write(&loam_key, "loam_test").unwrap();
    let args = [
        "--owner",
        OWNER,
        "--tailscale-socket",
        "/nonexistent/sock",
        "--loam-url",
        "http://127.0.0.1:9",
        "--loam-key-file",
        loam_key.to_str().unwrap(),
    ];
    let (h, cfg) = (home.display().to_string(), home.join(".config").display().to_string());
    let d = Daemon::child_labs_env(&args, &[("HOME", h.as_str()), ("XDG_CONFIG_HOME", cfg.as_str())]);
    let p = &d.get("/api/panes")[0];
    let (owner_pane, session) = (p["id"].as_u64().unwrap(), p["session"].as_u64().unwrap());
    d.post(&format!("/api/panes/{owner_pane}/send"), json!({ "text": "echo OWNERS-$((40+2))-LINE", "enter": true }));
    d.wait_for("the owner's echo", || {
        d.get(&format!("/api/panes/{owner_pane}/capture")).as_str().unwrap_or_default().contains("OWNERS-42-LINE")
    });

    // FRIEND edits the session and starts an agent beside the owner's pane.
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "editor" }));
    let config = json!({ "agent": "acp", "command": ["python3", fake()], "cwd": "/tmp", "prompt": "hi" });
    let (status, body) = d.raw_as(
        FRIEND,
        "POST",
        "/api/blocks",
        Some(json!({ "type": "agent", "config": config, "split": owner_pane })),
    );
    assert_eq!(status, 200, "{body}");
    let agent = serde_json::from_str::<Value>(&body).unwrap()["block"].as_u64().unwrap();
    let panes = d.get("/api/panes");
    let row = |id: u64| panes.as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap();
    assert_eq!(row(owner_pane)["tab"], row(agent)["tab"], "in the owner's tab");
    assert!(row(agent)["host"].is_u64(), "on a VM: {}", row(agent));

    let token = block_token(&d, agent);
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let refused = |tool: &str, args: Value| {
        let r = call(&rt, d.port, &token, tool, args.clone());
        let why = r.expect_err(&format!("{tool} {args} is refused"));
        assert!(why.contains("doesn't run on this agent's machine"), "{tool} {args}: {why}");
    };
    refused("read_file", json!({ "pane": owner_pane, "path": "~/notes.txt" }));
    refused("read_output", json!({ "pane": owner_pane, "offset": 0 }));
    refused("read_output", json!({ "pane": owner_pane, "screen": true }));
    let h = call(&rt, d.port, &token, "history", json!({ "pane": owner_pane })).unwrap();
    assert_eq!(h["commands"], json!([]), "{h}");
    let h = call(&rt, d.port, &token, "history", json!({ "kind": "output", "pattern": "OWNERS-42" })).unwrap();
    assert_eq!(h["hits"], json!([]), "{h}");
    // Its own pane's files are its VM's, not this host's.
    let own = call(&rt, d.port, &token, "read_file", json!({ "path": "~/notes.txt" }));
    assert!(!format!("{own:?}").contains("OWNERS-NOTES"), "{own:?}");

    // The owner's own agents still read the owner's panes (a full token).
    let full = d.post("/api/mcp/tokens", json!({ "name": "laptop" }))["token"].as_str().unwrap().to_owned();
    let r = call(&rt, d.port, &full, "read_output", json!({ "pane": owner_pane, "offset": 0 })).unwrap();
    assert!(r["text"].as_str().unwrap().contains("OWNERS-42-LINE"), "{r}");
    drop(d);
    let _ = std::fs::remove_dir_all(&home);
}
