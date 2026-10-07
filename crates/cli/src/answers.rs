//! What the CLI parses the daemon's answers into, against an older daemon's
//! answer: each type here is one the CLI reads with `Response::parse`, and
//! each fixture is the answer with every field the CLI used to read with a
//! default left out (the `json!` the daemon built before #446 had them; an
//! older one had fewer still). The parse must go through, with the default.

use arugula_proto::{
    Attention,
    api::*,
    fs::{FsEntry, FsList},
    hosts::{HostList, HostToken, Invite, SandboxList, Transport},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::http::parse_body;

/// `Response::parse` on a 200 with this body.
fn old<T: DeserializeOwned>(body: Value) -> T {
    let (typed, raw) = parse_body::<T>(200, "GET /api/x", &body.to_string()).unwrap_or_else(|e| panic!("{e:#}"));
    assert_eq!(raw, body, "the raw answer is what the daemon sent");
    typed
}

#[test]
fn a_non_2xx_answer_is_the_apis_error() {
    let err = |status, body: &str| parse_body::<Empty>(status, "POST /api/panes/9/send", body).unwrap_err().to_string();
    assert_eq!(err(404, r#"{"error":"no pane %9"}"#), "no pane %9");
    // No `error`: the body as it came.
    assert_eq!(err(400, r#"{"nope":1}"#), r#"{"nope":1}"#);
    assert_eq!(err(500, "boom"), r#""boom""#);
}

#[test]
fn an_answer_of_the_wrong_shape_names_the_route() {
    let err = parse_body::<RunResponse>(200, "POST /api/run", r#"{"oops":1}"#).unwrap_err().to_string();
    assert!(err.starts_with("POST /api/run: the daemon's answer isn't what this arugula expects"), "{err}");
    assert!(err.contains("missing field `pane`"), "{err}");
    let err = parse_body::<RunResponse>(200, "POST /api/run", "not json").unwrap_err().to_string();
    assert!(err.starts_with("POST /api/run:"), "{err}");
}

#[test]
fn answers_keep_what_a_newer_daemon_adds() {
    let run: RunResponse = old(json!({ "pane": 4, "tab": 2, "something": { "new": true } }));
    assert_eq!(run.pane, 4);
    let _: Empty = old(json!({ "note": "a newer daemon says more" }));
}

#[test]
fn empty_and_open_answers() {
    let _: Empty = old(json!({}));
    assert_eq!(old::<OpenResponse>(json!({ "block": 7 })).block, 7);
    let c: OpenConversationResponse = old(json!({ "block": 7, "opened": false, "conversation": "abc" }));
    assert_eq!((c.block, c.error), (7, None));
}

#[test]
fn waits_and_prompts() {
    assert_eq!(old::<WaitResult>(json!({ "result": "exit", "code": 3 })), WaitResult::Exit { code: Some(3) });
    assert_eq!(old::<WaitResult>(json!({ "result": "exit", "code": null })), WaitResult::Exit { code: None });
    assert_eq!(old::<WaitResult>(json!({ "result": "timeout" })), WaitResult::Timeout);
    let end: WaitResult = old(json!({ "result": "command_end", "text": null, "exit": null, "start": 0, "end": null }));
    assert!(matches!(end, WaitResult::CommandEnd { exit: None, .. }));
    let idle: WaitResult = old(json!({ "result": "attention", "state": "needs_input" }));
    assert!(matches!(idle, WaitResult::Attention { state: Attention::NeedsInput, ask: None }));
    // `send --wait`: a stall with neither the reason nor the screen.
    let stalled: PromptResult = old(json!({ "result": "stalled" }));
    assert_eq!(stalled, PromptResult::Stalled { why: String::new(), screen: String::new() });
    assert_eq!(
        old::<PromptResult>(json!({ "result": "needs_input" })),
        PromptResult::NeedsInput { question: None, ask: None }
    );
}

#[test]
fn a_process_with_no_argv_or_foreground() {
    let p: Process = old(json!({ "pid": 4, "comm": "zsh", "exe": null, "cwd": null }));
    assert_eq!((p.foreground, p.argv.len(), p.cwd), (0, 0, None));
}

#[test]
fn history_and_search_without_the_fields_the_cli_defaults() {
    // Before `open` and `started_ms` were certain, and `kind` didn't exist.
    let h: Vec<HistoryEntry> =
        old(json!([{ "pane": 3, "text": "ls", "cwd": null, "exit": 0, "start": 10, "end": 20, "ended_ms": null }]));
    assert!(h[0].open, "a pane with no `open` is open");
    assert_eq!(h[0].started_ms, 0);
    assert!(h[0].kind.is_command());
    let hits: Vec<SearchHit> = old(json!([{ "pane": 3, "open": true, "offset": 5, "command": null }]));
    assert_eq!(hits[0].line, "");
    let drivers: Vec<DriverEntry> = old(json!([{}]));
    assert_eq!((drivers[0].at_ms, drivers[0].offset, drivers[0].who.as_str()), (0, 0, ""));
}

#[test]
fn attention_without_a_headline() {
    let items: Vec<AttentionItem> = old(json!([{
        "pane": 2, "state": "needs_input",
        "reason": { "kind": "input", "since_ms": 5, "actions": ["dismiss"] },
    }]));
    assert_eq!(items[0].reason.headline, "");
}

#[test]
fn an_invite_without_the_grants_name() {
    let i: Invited = old(json!({
        "invite": "x", "grant": { "session": 1, "principal": "tailnet:a@b", "role": "viewer" },
        "pane": 2, "delivery": "sent", "reason": null, "drive": null,
    }));
    assert_eq!((i.grant.name.as_str(), i.grant.granted), ("", false));
}

#[test]
fn shares_and_guest_invites_without_times_or_flags() {
    let s: Vec<Share> = old(json!([{ "id": 1, "pane": 3 }]));
    assert_eq!((s[0].created_ms, s[0].expires_ms), (0, 0));
    let g: Vec<GuestInvite> = old(json!([{ "id": 1, "pane": 3 }]));
    assert_eq!((g[0].label.as_str(), g[0].rw, g[0].reusable, g[0].expires_ms), ("", false, false, 0));
    let _: Share =
        old(json!({ "id": 1, "pane": 3, "created_ms": 1, "expires_ms": 2, "token": "t", "path": "/share/t" }));
}

#[test]
fn studio_without_names_followers_or_blocks() {
    let st: StudioStatus = old(json!({ "url": "https://studio.example" }));
    assert_eq!((st.logged_in, st.followers.len()), (false, 0));
    let apps: StudioApps = old(json!({ "studio": null, "apps": [{ "title": null, "status": null }] }));
    assert_eq!((apps.apps[0].app.name.as_str(), apps.apps[0].blocks.len()), ("", 0));
    let l: StudioLoggedIn = old(json!({ "apps": [{}] }));
    assert_eq!(l.apps.len(), 1);
}

#[test]
fn ide_rules_and_shell_env_with_nothing_but_their_basics() {
    let ide: IdeInfo = old(json!({}));
    assert!(!ide.on, "no `on` is off");
    let ide: IdeInfo = old(json!({ "on": true, "others": [{ "pid": null }] }));
    let o = &ide.others.unwrap()[0];
    assert_eq!((o.name.as_str(), o.port, o.alive, o.folders.len()), ("", 0, false, 0));
    let r: Rules = old(json!({}));
    assert!(r.rules.is_empty());
    let r: Rules = old(json!({ "rules": [{ "tool": "Bash", "index": 2 }] }));
    assert_eq!((r.rules[0].index, r.rules[0].text.as_str()), (2, ""));
    let e: ShellEnv = old(json!({ "ok": true, "error": null, "path": null }));
    assert_eq!((e.shell.as_str(), e.ms, e.vars.len()), ("", 0, 0));
}

#[test]
fn the_agents_inventory_and_detection_of_an_older_daemon() {
    // Before the screen rules were reported: just chant's record.
    let inv: AgentsInventory = old(json!({ "state": "off", "sites": [] }));
    assert!(inv.rules.run.is_empty() && inv.rules.off.is_empty());
    assert_eq!(inv.inventory["state"], "off");
    let d: DetectionAnswer = old(json!({ "agent": "claude", "shown": null, "fired": null }));
    let DetectionAnswer::Read(d) = d else { panic!("{d:?}") };
    assert_eq!((d.name.as_str(), d.title.as_str(), d.rules.len(), d.unread), ("", "", 0, false));
    let d: DetectionAnswer = old(json!({
        "agent": "claude", "shown": null, "fired": null,
        "rules": [{}],
    }));
    let DetectionAnswer::Read(d) = d else { panic!("{d:?}") };
    assert!(!d.rules[0].matched && d.rules[0].text.is_empty());
    assert!(matches!(old::<DetectionAnswer>(json!({ "agent": null, "command": "vim" })), DetectionAnswer::NoAgent(_)));
}

#[test]
fn conversations_adapters_and_fountain() {
    let l: ConversationList = old(json!({}));
    assert!(l.conversations.is_empty());
    let l: ConversationList = old(json!({ "conversations": [{ "id": "abc", "title": "t" }], "total": 1 }));
    assert_eq!((l.conversations[0].conversation["id"].as_str(), l.conversations[0].block), (Some("abc"), None));
    let a: Adapters = old(json!({ "adapters": [{ "kind": "claude" }] }));
    assert_eq!(a.adapters.len(), 1);
    let f: FountainAgents = old(json!({ "profile": "default", "filter": {} }));
    assert_eq!((f.base_url.as_str(), f.total, f.agents.len(), f.unreadable), ("", 0, 0, 0));
}

#[test]
fn machines_synced_hosts_and_act_results_of_an_older_daemon() {
    let m: Vec<arugula_proto::Machine> = old(json!([{ "id": 1, "owner": { "pane": 3 } }]));
    assert_eq!((m[0].provider.as_str(), m[0].sprite.as_str()), ("", ""));
    let h: Vec<SyncedHost> = old(json!([{}]));
    assert_eq!((h[0].name.as_str(), h[0].panes.len()), ("", 0));
    let a: ActResponse = old(json!({}));
    assert!(a.results.is_empty());
}

#[test]
fn hosts_sandboxes_and_tokens_of_an_older_daemon() {
    let l: HostList = old(json!({}));
    assert_eq!((l.this.as_str(), l.hosts.len()), ("", 0));
    let l: HostList = old(json!({ "this": "home", "hosts": [{ "provider": { "port": 7681 } }] }));
    let h = &l.hosts[0];
    assert_eq!((h.name.as_str(), h.urls.len(), h.transport), ("", 0, Transport::Tailnet));
    assert_eq!(h.provider.as_ref().map(|p| p.provider.as_str()), Some(""));
    let s: SandboxList = old(json!({ "provider": { "resident": false } }));
    assert_eq!((s.provider.name.as_str(), s.provider.exec_replay, s.sandboxes.len()), ("", 0, 0));
    let s: SandboxList = old(json!({ "provider": { "resident": true }, "sandboxes": [{}] }));
    assert_eq!(s.sandboxes[0].status, "");
    assert_eq!(old::<Invite>(json!({ "expires_ms": 1 })).token, "");
    assert_eq!(old::<HostToken>(json!({ "name": "x" })).token, "");
}

#[test]
fn files_of_an_older_daemon() {
    let e: FsEntry = old(json!({ "type": "file", "mtime_ms": 0 }));
    assert_eq!((e.name.as_str(), e.path.as_str(), e.size, e.mode), ("", "", 0, 0));
    let l: FsList = old(
        json!({ "path": "/", "parent": null, "entries": [{ "type": "symlink", "target": "directory", "mtime_ms": 0 }] }),
    );
    assert!(l.entries[0].is_dir() && !l.truncated);
}

#[test]
fn hook_answers_without_their_output() {
    // `ask` and `hook` print `output` as it came; a daemon that left it out
    // gave `null` there before, too.
    assert!(matches!(old::<AskAnswer>(json!({ "action": "accept" })), AskAnswer::Accept { .. }));
    assert!(matches!(old::<AskAnswer>(json!({ "action": "decline" })), AskAnswer::Decline { .. }));
    assert_eq!(old::<AskAnswer>(json!({ "action": "terminal" })), AskAnswer::Terminal);
    assert!(matches!(old::<PermitAnswer>(json!({ "action": "allow" })), PermitAnswer::Allow { .. }));
    assert!(matches!(old::<PermitAnswer>(json!({ "action": "deny" })), PermitAnswer::Deny { .. }));
    // `inbox`: a follow-up with no sender or text.
    let f: InboxAnswer = old(json!({ "action": "follow_up" }));
    assert!(matches!(f, InboxAnswer::FollowUp { ref by, .. } if by.name.is_empty()));
    assert_eq!(old::<InboxAnswer>(json!({ "action": "replaced" })), InboxAnswer::Replaced);
}

/// A request body, as the typed request writes it and as the `json!` it
/// replaced wrote it: the same, or the same plus `extra` (keys the daemon
/// reads with a default, written with it).
fn body<T: serde::Serialize>(typed: T, old: Value, extra: &[(&str, Value)]) {
    let mut old = old;
    for (k, v) in extra {
        old[*k] = v.clone();
    }
    assert_eq!(serde_json::to_value(typed).unwrap(), old);
}

#[test]
fn request_bodies_are_what_the_json_was() {
    use arugula_core::Role;
    use arugula_proto::{BlockType, Policy};

    // `run`: nothing added.
    body(
        RunRequest {
            command: Some("ls".into()),
            vm: false,
            vm_tab: true,
            image: None,
            sandbox: None,
            session: Some("s".into()),
            split: Some(2),
            join: false,
            cwd: None,
            policy: Some(Policy::Rerun { confirm: true }),
            from_pane: Some(1),
        },
        json!({ "command": "ls", "vm": false, "vm_tab": true, "image": null, "sandbox": null, "session": "s",
                "split": 2, "join": false, "cwd": null, "policy": { "kind": "rerun", "confirm": true },
                "from_pane": 1 }),
        &[],
    );
    // `run --home`'s first request never named `vm_tab`, `join`, `split` or `from_pane`.
    body(
        RunRequest { command: Some("ls".into()), session: Some("me".into()), ..Default::default() },
        json!({ "command": "ls", "vm": false, "image": null, "sandbox": null, "session": "me", "cwd": null,
                "policy": null }),
        &[("vm_tab", json!(false)), ("join", json!(false)), ("split", Value::Null), ("from_pane", Value::Null)],
    );
    body(SendRequest { text: "x".into(), enter: true }, json!({ "text": "x", "enter": true }), &[]);
    body(KeysRequest { keys: vec!["C-c".into()] }, json!({ "keys": ["C-c"] }), &[]);
    body(
        PromptRequest { text: "go".into(), answering: false, stall: None, timeout: Some(5.0) },
        json!({ "text": "go", "answering": false, "timeout": 5.0 }),
        &[("stall", Value::Null)],
    );
    body(
        InviteRequest {
            session: 1,
            who: "sam".into(),
            role: Some(Role::Editor),
            note: None,
            pane: Some(3),
            history: false,
            drive_minutes: Some(10),
            root: None,
            thread: None,
            msg: None,
            whole_thread: false,
        },
        json!({ "session": 1, "who": "sam", "role": "editor", "pane": 3, "history": false, "drive_minutes": 10,
                "note": null, "root": null }),
        &[("thread", Value::Null), ("msg", Value::Null), ("whole_thread", json!(false))],
    );
    body(ShareRequest { pane: 3, ttl_secs: Some(60) }, json!({ "pane": 3, "ttl_secs": 60 }), &[]);
    body(
        GuestInviteRequest {
            pane: 3,
            ttl_secs: Some(60),
            rw: true,
            reusable: false,
            label: None,
            host: None,
            relay: Some(true),
        },
        json!({ "pane": 3, "ttl_secs": 60, "rw": true, "reusable": false, "label": null, "host": null,
                "relay": true }),
        &[],
    );
    body(
        OpenConversationRequest { id: "abc".into(), then: None, session: None, split: None, from_pane: Some(2) },
        json!({ "id": "abc", "session": null, "split": null, "from_pane": 2 }),
        &[("then", Value::Null)],
    );
    // Every block: `image` too, which the daemon reads as none.
    body(
        OpenRequest {
            kind: BlockType::Browser,
            config: json!({ "port": 3000 }),
            split: None,
            host: Some(2),
            local: false,
            session: None,
            from_pane: Some(1),
            ..Default::default()
        },
        json!({ "type": "browser", "config": { "port": 3000 }, "split": null, "host": 2, "local": false,
                "session": null, "from_pane": 1 }),
        &[("vm", json!(false)), ("image", Value::Null)],
    );
    body(
        AskRequest { questions: json!([{ "q": 1 }]), id: Some("t".into()), source: None, agent: None },
        json!({ "questions": [{ "q": 1 }], "id": "t" }),
        &[("source", Value::Null), ("agent", Value::Null)],
    );
    body(WithdrawRequest { id: None }, json!({ "id": null }), &[]);
    body(IdeDiffsRequest { diffs: "arugula".into() }, json!({ "diffs": "arugula" }), &[]);
    body(
        StudioLoginRequest { url: "https://s".into(), token: "t".into() },
        json!({ "url": "https://s", "token": "t" }),
        &[],
    );
    body(FollowerLinkRequest { link: "l".into() }, json!({ "link": "l" }), &[]);
    body(
        ActRequest {
            action: arugula_proto::Action::Rerun,
            pane: Some(2),
            panes: vec![],
            id: None,
            content: None,
            option: None,
            suggestion: None,
            message: None,
            text: None,
        },
        json!({ "action": "rerun", "pane": 2 }),
        &[],
    );
    body(arugula_proto::fs::CdRequest { path: "/tmp".into() }, json!({ "path": "/tmp" }), &[]);
    body(
        arugula_proto::hosts::AddHost {
            name: "b".into(),
            urls: vec![],
            transport: Transport::Ssh,
            ssh: Some("u@b".into()),
        },
        json!({ "name": "b", "urls": [], "transport": "ssh", "ssh": "u@b" }),
        &[],
    );
    body(
        arugula_proto::hosts::AddHost {
            name: "b".into(),
            urls: vec!["https://b".into()],
            transport: Transport::Tailnet,
            ssh: None,
        },
        json!({ "name": "b", "urls": ["https://b"], "transport": "tailnet" }),
        &[],
    );
    body(
        arugula_proto::hosts::PromoteRequest { host: None, port: Some(7681) },
        json!({ "host": null, "port": 7681 }),
        &[],
    );
}
