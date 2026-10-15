//! The link's mapping, without I/O: the Go bridge's cases
//! (`cli/internal/acp/*_test.go`), ported, and what a link that follows a
//! conversation (rather than one turn) adds.

use serde_json::{Value, json};

use super::*;

fn core() -> Core {
    let mut c = Core::new(7, true);
    c.conversation = Some("conv-1".into());
    c
}

fn stage(id: u64, stage: &str, state: &str, meta: Value) -> Ev {
    Ev {
        id,
        kind: "stage".into(),
        data: meta.to_string(),
        stage: stage.into(),
        state: state.into(),
        turn_id: meta["turn_id"].as_str().map(str::to_owned),
        ..Default::default()
    }
}

fn output(id: u64, stream: &str, line: &str) -> Ev {
    Ev { id, kind: "output".into(), stream: stream.into(), data: line.into(), ..Default::default() }
}

fn acp(id: u64, line: Value) -> Ev {
    output(id, "acp", &line.to_string())
}

/// What the sandbox's adapter said: an update, under its own session id.
fn said(text: &str) -> Value {
    json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "sprite-session",
        "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } } } })
}

fn prompt_frame(rpc: u64, text: &str, crid: Option<&str>) -> Value {
    let mut p = json!({ "sessionId": "conv-1", "prompt": [{ "type": "text", "text": text }] });
    if let Some(c) = crid {
        p["_meta"] = json!({ "clientRequestId": c });
    }
    json!({ "jsonrpc": "2.0", "id": rpc, "method": "session/prompt", "params": p })
}

/// Send a prompt: the call it makes.
fn prompt(c: &mut Core, rpc: u64, crid: &str) -> Call {
    let (frames, calls) = c.outbound(&prompt_frame(rpc, "go", Some(crid)));
    assert!(frames.is_empty(), "{frames:?}");
    calls.into_iter().next().expect("a call")
}

fn started(id: u64, turn: &str, crid: Option<&str>) -> Ev {
    let mut m = json!({ "turn_id": turn, "turn_number": 1 });
    if let Some(c) = crid {
        m["client_request_id"] = json!(c);
    }
    stage(id, "turn", "started", m)
}

/// The answer to `rpc` among `frames`.
fn answer(frames: &[Value], rpc: u64) -> Option<Value> {
    frames.iter().find(|f| f["id"] == rpc && f.get("method").is_none()).cloned()
}

fn methods(frames: &[Value]) -> Vec<String> {
    frames.iter().filter_map(|f| f["method"].as_str().map(str::to_owned)).collect()
}

fn conflict(code: &str) -> Error {
    Error::Conflict { code: code.into(), message: String::new() }
}

// ------------------------------------------------------------ prompts

#[test]
fn a_prompt_forwards_updates_and_returns_the_stop_reason() {
    let mut c = core();
    let call = prompt(&mut c, 3, "arugula-7-3");
    assert_eq!(
        call,
        Call::Prompt { rpc: json!(3), text: "go".into(), images: vec![], crid: Some("arugula-7-3".into()) }
    );
    assert!(c.event(&started(10, "t1", Some("arugula-7-3"))).iter().all(|f| f["method"] == STATUS));
    let f = c.event(&acp(11, said("hello")));
    assert_eq!(f.len(), 1);
    assert_eq!(f[0]["method"], "session/update");
    // Our session id, not the sandbox's.
    assert_eq!(f[0]["params"]["sessionId"], "conv-1");
    assert_eq!(f[0]["params"]["update"]["content"]["text"], "hello");
    assert_eq!(event_of(&f[0]), Some(11));
    let f = c.event(&stage(12, "turn", "done", json!({ "turn_id": "t1", "stop_reason": "end_turn" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "end_turn");
    // The event's id on its last frame only.
    assert_eq!(event_of(f.last().unwrap()), Some(12));
    assert!(f[..f.len() - 1].iter().all(|x| event_of(x).is_none()));
    assert!(c.waiting().is_none());
}

#[test]
fn a_client_request_id_belongs_to_each_submission() {
    let mut c = core();
    let Call::Prompt { crid, .. } = prompt(&mut c, 3, "a") else { panic!() };
    assert_eq!(crid.as_deref(), Some("a"));
    c.event(&stage(1, "turn", "done", json!({ "stop_reason": "end_turn" })));
    let Call::Prompt { crid, .. } = prompt(&mut c, 4, "b") else { panic!() };
    assert_eq!(crid.as_deref(), Some("b"));
    // None sends none.
    let (_, calls) = c.outbound(&prompt_frame(5, "go", None));
    assert!(matches!(&calls[0], Call::Prompt { crid: None, .. }));
    // Not a string: refused, not sent.
    let mut bad = prompt_frame(6, "go", None);
    bad["params"]["_meta"] = json!({ "clientRequestId": 7 });
    let (f, calls) = c.outbound(&bad);
    assert!(calls.is_empty());
    assert_eq!(f[0]["error"]["code"], INVALID_PARAMS);
}

/// #398 from the other side: another turn's end doesn't end ours. (The Go
/// bridge learns the stream head before posting; a link that follows the
/// conversation all along tells turns apart by the request id instead.)
#[test]
fn another_turns_end_doesnt_end_ours() {
    let mut c = core();
    prompt(&mut c, 3, "ours");
    // Another client's turn, started and ended.
    c.event(&started(10, "theirs", Some("someone-else")));
    let f = c.event(&stage(11, "turn", "done", json!({ "turn_id": "theirs" })));
    assert!(answer(&f, 3).is_none(), "{f:?}");
    assert!(c.waiting().is_some());
    c.event(&started(12, "t2", Some("ours")));
    let f = c.event(&stage(13, "turn", "done", json!({ "turn_id": "t2" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "end_turn");
}

#[test]
fn a_refusal_is_a_stop_reason_not_an_error() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    c.event(&started(1, "t", Some("r")));
    let f = c.event(&stage(2, "turn", "failed", json!({ "turn_id": "t", "stop_reason": "refusal" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "refusal");
}

#[test]
fn an_interrupted_turn_is_cancelled() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    c.event(&started(1, "t", Some("r")));
    let f = c.event(&stage(2, "turn", "interrupted", json!({ "turn_id": "t" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "cancelled");
}

/// What ACP has no word for is an error, never "the agent finished".
#[test]
fn infrastructure_failures_are_errors_not_stop_reasons() {
    for (ev, want) in [
        (stage(2, "provision", "failed", json!({ "reason": "quota" })), "sandbox never started: quota"),
        (stage(2, "turn", "failed", json!({ "turn_id": "t", "error": "peer_down" })), "peer_down"),
        (stage(2, "sandbox", "done", json!({ "reason": "max_lifetime" })), "reclaimed mid-turn: max_lifetime"),
        (stage(2, "terminate", "done", json!({})), "terminated"),
        (stage(2, "reattach", "failed", json!({})), "prompt again to provision a fresh one"),
    ] {
        let mut c = core();
        prompt(&mut c, 3, "r");
        c.event(&started(1, "t", Some("r")));
        let f = c.event(&ev);
        let a = answer(&f, 3).unwrap_or_else(|| panic!("{ev:?}: no answer in {f:?}"));
        let m = a["error"]["message"].as_str().unwrap_or_default();
        assert!(m.contains(want), "{ev:?}: {m}");
        assert_eq!(a["error"]["code"], INTERNAL);
    }
}

/// A refused admission is before any turn: an error that says to retry,
/// and doesn't claim a reclaim.
#[test]
fn a_refused_admission_is_an_error_with_a_retry_hint() {
    for (event, reason) in [("admission_refused", "sandbox_unavailable"), ("at_capacity", "sandbox_at_capacity")] {
        let mut c = core();
        prompt(&mut c, 3, "r");
        let f = c.event(&stage(1, "sandbox", "done", json!({ "event": event, "reason": reason })));
        let m = answer(&f, 3).unwrap()["error"]["message"].as_str().unwrap().to_owned();
        for want in ["prompt was not started", reason, "retry shortly"] {
            assert!(m.contains(want), "{event}: {m}");
        }
        assert!(!m.contains("reclaimed"), "{m}");
        // The turn/done after it has nothing left to answer.
        assert!(answer(&c.event(&stage(2, "turn", "done", json!({}))), 3).is_none());
    }
}

/// A suspend keeps the machine: an end, not a failure.
#[test]
fn a_suspended_sandbox_ends_the_turn_cleanly() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    c.event(&started(1, "t", Some("r")));
    let f = c.event(&stage(2, "sandbox", "done", json!({ "event": "suspended", "reason": "idle" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "end_turn");
    let st = f.iter().find(|x| x["method"] == STATUS).unwrap();
    assert_eq!(st["params"]["machine"], "parked");
}

#[test]
fn non_protocol_output_and_the_sandboxs_own_answers_are_not_forwarded() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    assert!(c.event(&output(1, "stderr", "npm warn deprecated foo@1.0.0")).is_empty());
    assert!(c.event(&output(2, "stdout", "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\"}")).is_empty());
    // An answer the sandbox's adapter gave Fountain is not an answer to us.
    assert!(c.event(&acp(3, json!({ "jsonrpc": "2.0", "id": 3, "result": { "stopReason": "end_turn" } }))).is_empty());
    assert!(c.event(&output(4, "acp", "not json")).is_empty());
    assert!(c.waiting().is_some());
    // Nothing was made, yet each moved the cursor: none comes again.
    assert_eq!(c.cursor, 4);
}

/// #724: a refused model doesn't end the turn: the runtime answers on its
/// default.
#[test]
fn a_refused_model_does_not_end_the_turn() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    c.event(&started(1, "t", Some("r")));
    let f =
        c.event(&stage(2, "model", "failed", json!({ "requested": "claude-sonnet-4-6", "detail": "Invalid value" })));
    assert!(answer(&f, 3).is_none());
    assert_eq!(methods(&c.event(&acp(3, said("answered anyway")))), ["session/update"]);
    let f = c.event(&stage(4, "turn", "done", json!({ "turn_id": "t", "stop_reason": "end_turn" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "end_turn");
}

#[test]
fn a_turn_that_failed_before_it_started_ends_the_prompt() {
    // Fountain's fail_before_start: a failed turn with no started stage.
    let mut c = core();
    prompt(&mut c, 3, "r");
    let f = c.event(&stage(1, "turn", "failed", json!({ "turn_id": "t9", "reason": "spawn failed" })));
    assert!(answer(&f, 3).unwrap()["error"]["message"].as_str().unwrap().contains("spawn failed"));
}

#[test]
fn a_prompt_with_nothing_usable_is_refused_and_images_go_with_it() {
    let mut c = core();
    let empty = json!({ "jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {
        "sessionId": "conv-1", "prompt": [{ "type": "resource_link", "uri": "file:///etc/passwd", "name": "p" }] } });
    let (f, calls) = c.outbound(&empty);
    assert!(calls.is_empty());
    assert!(f[0]["error"]["message"].as_str().unwrap().contains("no text or image"));
    // A local resource is dropped, not sent as a path; an image goes.
    let p = json!({ "jsonrpc": "2.0", "id": 4, "method": "session/prompt", "params": { "sessionId": "conv-1", "prompt": [
        { "type": "text", "text": "look" },
        { "type": "resource_link", "uri": "file:///home/me/x.rs", "name": "x.rs" },
        { "type": "image", "data": "aGk=", "mimeType": "image/png" },
        { "type": "text", "text": "here" },
    ] } });
    let (_, calls) = c.outbound(&p);
    assert_eq!(
        calls,
        [Call::Prompt {
            rpc: json!(4),
            text: "look\nhere".into(),
            images: vec![("aGk=".into(), "image/png".into())],
            crid: None
        }]
    );
}

#[test]
fn a_prompt_that_cant_be_sent_says_whether_to_retry() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    let f = c.prompt_failed(&json!(3), &conflict("conversation_busy"));
    assert!(f[0]["error"]["message"].as_str().unwrap().contains("retry shortly"), "{f:?}");
    assert!(c.waiting().is_none());
    prompt(&mut c, 4, "s");
    let bad = Error::Invalid { status: 422, code: "invalid_prompt".into(), message: "blank".into(), fields: None };
    let f = c.prompt_failed(&json!(4), &bad);
    assert!(!f[0]["error"]["message"].as_str().unwrap().contains("retry"), "{f:?}");
}

// ------------------------------------------------------------- wakes

/// A prompt answered `queued` whose wake failed: no turn opens for it, and
/// `retryable` says whether sending it again can work.
#[test]
fn a_failed_wake_ends_the_prompt_retryable_or_not() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    let f = c.event(&stage(1, "wake", "failed", json!({ "reason": "fleet_full", "retryable": true })));
    let m = answer(&f, 3).unwrap()["error"]["message"].as_str().unwrap().to_owned();
    assert!(m.contains("fleet_full") && m.contains("retry shortly"), "{m}");

    prompt(&mut c, 4, "s");
    let f = c.event(&stage(2, "wake", "failed", json!({ "reason": "agent_missing", "retryable": false })));
    let m = answer(&f, 4).unwrap()["error"]["message"].as_str().unwrap().to_owned();
    assert!(m.contains("agent_missing") && !m.contains("retry"), "{m}");
}

/// Two clients prompting a parked conversation: both are told `queued`,
/// and Fountain refuses the second on the stream alone.
#[test]
fn a_prompt_refused_on_the_stream_ends_with_a_retry() {
    let mut c = core();
    prompt(&mut c, 3, "ours");
    // The other client's prompt opened the turn.
    assert!(answer(&c.event(&started(1, "theirs", Some("theirs"))), 3).is_none());
    let f = c.event(&stage(2, "wake", "failed", json!({ "reason": "conversation_busy", "retryable": true })));
    let m = answer(&f, 3).unwrap()["error"]["message"].as_str().unwrap().to_owned();
    assert!(m.contains("conversation_busy") && m.contains("retry shortly"), "{m}");
    // Their turn's end is theirs.
    assert!(c.event(&stage(3, "turn", "done", json!({ "turn_id": "theirs" }))).iter().all(|f| f.get("id").is_none()));
}

#[test]
fn a_failed_wake_leaves_a_running_turn_alone() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    c.event(&started(1, "t", Some("r")));
    assert!(answer(&c.event(&stage(2, "wake", "failed", json!({ "reason": "x" }))), 3).is_none());
    assert!(c.waiting().is_some());
}

// ------------------------------------------------------------ cancels

#[test]
fn cancel_interrupts_and_the_turns_end_answers_it() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    c.event(&started(1, "t", Some("r")));
    let (f, calls) =
        c.outbound(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": "conv-1" } }));
    // It answers nothing itself: the prompt does.
    assert!(f.is_empty());
    assert_eq!(calls, [Call::Interrupt]);
    assert!(c.interrupted(Ok(())).is_empty());
    let f = c.event(&stage(2, "turn", "interrupted", json!({ "turn_id": "t" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "cancelled");
}

/// A cancel that raced the turn's end (409 `no_turn_running`) isn't a
/// failure. A prompt that never got a turn is answered `cancelled`; one
/// whose turn ran waits for that turn's end, which is on its way.
#[test]
fn a_cancel_that_raced_the_end_is_not_an_error() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    let f = c.interrupted(Err(conflict("no_turn_running")));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "cancelled");
    assert!(c.waiting().is_none());

    prompt(&mut c, 4, "s");
    c.event(&started(1, "t", Some("s")));
    assert!(c.interrupted(Err(conflict("no_turn_running"))).is_empty());
    let f = c.event(&stage(2, "turn", "done", json!({ "turn_id": "t", "stop_reason": "end_turn" })));
    assert_eq!(answer(&f, 4).unwrap()["result"]["stopReason"], "end_turn");
}

#[test]
fn a_cancel_survives_a_failed_interrupt() {
    let mut c = core();
    prompt(&mut c, 3, "r");
    c.event(&started(1, "t", Some("r")));
    let e = Error::Status { status: 500, code: String::new(), message: "boom".into() };
    assert!(c.interrupted(Err(e)).is_empty());
    // The turn goes on, and its end answers the prompt.
    let f = c.event(&stage(2, "turn", "done", json!({ "turn_id": "t" })));
    assert_eq!(answer(&f, 3).unwrap()["result"]["stopReason"], "end_turn");
}

// -------------------------------------------------------- permissions

fn asked(id: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": {
        "sessionId": "sprite-session",
        "toolCall": { "toolCallId": "tc1", "title": "rm -rf build", "kind": "execute",
                      "locations": [{ "path": "/workspace/build" }] },
        "options": [
            { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
            { "optionId": "always", "name": "Always", "kind": "allow_always" },
            { "optionId": "reject", "name": "Reject", "kind": "reject_once" },
        ] } })
}

fn answered(rpc: Value, outcome: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": rpc, "result": { "outcome": outcome } })
}

#[test]
fn a_permission_request_reaches_the_block_and_its_answer_goes_back() {
    let mut c = core();
    let f = c.event(&acp(5, asked(json!(4))));
    assert_eq!(methods(&f), ["session/request_permission"]);
    assert_eq!(f[0]["id"], 4, "its own id: the block answers on it");
    assert_eq!(f[0]["params"]["sessionId"], "conv-1");
    // The sandbox's paths are not offered as this machine's.
    assert!(f[0]["params"]["toolCall"].get("locations").is_none());
    assert_eq!(f[0]["params"]["toolCall"]["_meta"][SANDBOX_LOCATIONS][0]["path"], "/workspace/build");
    assert_eq!(f[0]["params"]["toolCall"]["title"], "rm -rf build");
    let (_, calls) = c.outbound(&answered(json!(4), json!({ "outcome": "selected", "optionId": "allow" })));
    assert_eq!(calls, [Call::Answer { request: "4".into(), option: "allow".into() }]);
}

#[test]
fn a_dismissed_request_is_refused_with_the_agents_own_rejection() {
    let mut c = core();
    c.event(&acp(5, asked(json!(4))));
    let (_, calls) = c.outbound(&answered(json!(4), json!({ "outcome": "cancelled" })));
    assert_eq!(calls, [Call::Answer { request: "4".into(), option: "reject".into() }]);
}

#[test]
fn an_option_the_agent_never_offered_is_not_relayed() {
    let mut c = core();
    c.event(&acp(5, asked(json!(4))));
    let (_, calls) = c.outbound(&answered(json!(4), json!({ "outcome": "selected", "optionId": "made-up" })));
    assert_eq!(calls, [Call::Answer { request: "4".into(), option: "reject".into() }]);
}

#[test]
fn with_no_rejection_on_offer_nothing_is_sent() {
    let mut c = core();
    let mut a = asked(json!(4));
    a["params"]["options"] = json!([{ "optionId": "allow", "kind": "allow_once" }]);
    c.event(&acp(5, a));
    let (_, calls) = c.outbound(&answered(json!(4), json!({ "outcome": "cancelled" })));
    assert!(calls.is_empty(), "left for Fountain's timeout: {calls:?}");
}

#[test]
fn a_request_id_is_stringified_the_way_the_server_does() {
    let mut c = core();
    c.event(&acp(5, asked(json!("req-x"))));
    let (_, calls) = c.outbound(&answered(json!("req-x"), json!({ "outcome": "selected", "optionId": "allow" })));
    assert_eq!(calls, [Call::Answer { request: "req-x".into(), option: "allow".into() }]);
}

/// Answered anywhere (here, the phone, the CLI, the timeout): the card
/// goes.
#[test]
fn a_request_resolved_anywhere_withdraws_the_card() {
    let mut c = core();
    c.event(&acp(5, asked(json!(4))));
    let f =
        c.event(&stage(6, "request", "done", json!({ "request_id": 4, "outcome": "answered", "option_id": "allow" })));
    // (Then the status, told for the first time.)
    assert_eq!(methods(&f), ["$/cancel_request", STATUS]);
    assert_eq!(f[0]["params"]["requestId"], 4);
    assert_eq!(event_of(f.last().unwrap()), Some(6));
    let f = c.event(&stage(7, "request", "done", json!({ "request_id": "r2" })));
    assert_eq!(methods(&f), ["$/cancel_request"]);
    assert_eq!(event_of(&f[0]), Some(7));
}

#[test]
fn tool_call_locations_go_under_meta_and_nothing_else_changes() {
    let mut c = core();
    let tc = json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s", "update": {
        "sessionUpdate": "tool_call", "toolCallId": "tc", "title": "Read lib/foo.ex", "kind": "read",
        "rawInput": { "path": "/workspace/lib/foo.ex" }, "locations": [{ "path": "/workspace/lib/foo.ex", "line": 3 }] } } });
    let f = c.event(&acp(1, tc));
    let u = &f[0]["params"]["update"];
    assert!(u.get("locations").is_none());
    assert_eq!(u["_meta"][SANDBOX_LOCATIONS], json!([{ "path": "/workspace/lib/foo.ex", "line": 3 }]));
    assert_eq!(u["rawInput"]["path"], "/workspace/lib/foo.ex");
    assert_eq!(u["title"], "Read lib/foo.ex");
    // One without locations is left as it is.
    let f = c.event(&acp(2, said("x")));
    assert!(f[0]["params"]["update"].get("_meta").is_none());
}

// ------------------------------------------------- following, restarts

#[test]
fn initialize_offers_resume_only_with_a_cursor() {
    let (f, _) =
        Core::new(1, true).outbound(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }));
    let caps = &f[0]["result"]["agentCapabilities"];
    assert_eq!(caps["loadSession"], true);
    assert!(caps["sessionCapabilities"]["resume"].is_object());
    assert_eq!(caps["promptCapabilities"]["image"], true);
    assert_eq!(f[0]["result"]["agentInfo"]["name"], "fountain");
    // A block saved by a daemon that ran `fountain acp`: no cursor, so it
    // loads, once.
    let (f, _) = Core::new(1, false).outbound(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }));
    assert!(f[0]["result"]["agentCapabilities"]["sessionCapabilities"].get("resume").is_none());
}

#[test]
fn events_at_or_below_the_cursor_are_not_handed_on_twice() {
    let mut c = core();
    assert_eq!(c.event(&acp(5, said("a"))).len(), 1);
    assert!(c.event(&acp(5, said("a"))).is_empty());
    assert!(c.event(&acp(3, said("older"))).is_empty());
    assert_eq!(c.cursor, 5);
    // Unnumbered (the server stage): handed on, the cursor stays.
    c.event(&stage(0, "server", "done", json!({})));
    assert_eq!(c.cursor, 5);
}

/// A restarted daemon: the prompt it had sent is still waiting, bound to
/// the turn it opened, and that turn's end (which came while it was down)
/// answers it.
#[test]
fn an_adopted_link_answers_the_prompt_waiting_from_before() {
    let mut c = Core::new(7, true);
    c.adopt("conv-1".into(), 20, Some((json!(5), Some("arugula-7-5".into()))), Some("t1".into()));
    assert!(c.event(&acp(19, said("seen before"))).is_empty());
    let f = c.event(&acp(21, said("while it was down")));
    assert_eq!(f[0]["params"]["sessionId"], "conv-1");
    let f = c.event(&stage(22, "turn", "done", json!({ "turn_id": "t1", "stop_reason": "end_turn" })));
    assert_eq!(answer(&f, 5).unwrap()["result"]["stopReason"], "end_turn");
    // Unbound (its start wasn't seen): its own request id finds it.
    let mut c = Core::new(7, true);
    c.adopt("conv-1".into(), 20, Some((json!(5), Some("arugula-7-5".into()))), None);
    c.event(&started(21, "t2", Some("arugula-7-5")));
    let f = c.event(&stage(22, "turn", "done", json!({ "turn_id": "t2" })));
    assert!(answer(&f, 5).is_some());
}

#[test]
fn a_replay_is_updates_only() {
    let mut c = core();
    assert!(c.replayed(&acp(1, said("a"))).is_some_and(|f| event_of(&f) == Some(1)));
    // A request answered long ago, and the stages, aren't asked again.
    assert!(c.replayed(&acp(2, asked(json!(4)))).is_none());
    assert!(c.replayed(&stage(3, "turn", "done", json!({}))).is_none());
    assert_eq!(c.cursor, 3);
    // A turn's start is its prompt, when the turns said it.
    c.prompts.insert("t2".into(), "do it".into());
    let f = c.replayed(&started(4, "t2", None)).unwrap();
    assert_eq!(f["params"]["update"]["sessionUpdate"], "user_message_chunk");
    assert_eq!(f["params"]["update"]["content"]["text"], "do it");
    assert!(c.replayed(&started(5, "t3", None)).is_none());
}

#[test]
fn the_status_follows_the_stages() {
    let mut c = core();
    let st = |f: Vec<Value>| f.into_iter().find(|x| x["method"] == STATUS).map(|x| x["params"].clone());
    let s = st(c.event(&started(1, "t", None))).unwrap();
    assert_eq!(
        (s["status"].as_str(), s["machine"].as_str(), s["turn_id"].as_str()),
        (Some("running"), Some("awake"), Some("t"))
    );
    let s = st(c.event(&stage(2, "turn", "done", json!({ "turn_id": "t" })))).unwrap();
    assert_eq!((s["status"].as_str(), s["machine"].as_str()), (Some("idle"), Some("awake")));
    // Nothing new: nothing said.
    assert!(st(c.event(&stage(3, "config", "done", json!({})))).is_none());
    let s = st(c.event(&stage(4, "sandbox", "done", json!({ "event": "suspended" })))).unwrap();
    assert_eq!(s["machine"], "parked");
    assert_eq!(st(c.event(&stage(5, "reattach", "started", json!({})))).unwrap()["machine"], "waking");
    assert_eq!(st(c.event(&stage(6, "reattach", "done", json!({})))).unwrap()["machine"], "awake");
    let s = st(c.event(&stage(7, "terminate", "done", json!({})))).unwrap();
    assert_eq!((s["status"].as_str(), s["machine"].as_str()), (Some("terminated"), Some("none")));
}

#[test]
fn permission_policies_parse_as_fountain_acp_takes_them() {
    assert_eq!(permission_policy("ask").unwrap(), Some(json!({ "default": "ask" })));
    assert_eq!(
        permission_policy("auto_allow, execute=ask").unwrap(),
        Some(json!({ "default": "auto_allow", "execute": "ask" }))
    );
    assert_eq!(permission_policy("").unwrap(), None);
    assert!(permission_policy("execute=maybe").is_err());
    assert!(permission_policy("=ask").is_err());
}

#[test]
fn what_fountain_doesnt_do_is_answered() {
    let mut c = core();
    let (f, _) = c.outbound(&json!({ "jsonrpc": "2.0", "id": 9, "method": "session/set_config_option", "params": {} }));
    assert_eq!(f[0]["result"]["_meta"]["fountain"]["applied"], false);
    let (f, _) = c.outbound(&json!({ "jsonrpc": "2.0", "id": 10, "method": "session/fork", "params": {} }));
    assert_eq!(f[0]["error"]["code"], NOT_FOUND);
}
