//! `run`: a run's eight states and the moves between them, the line a
//! daemon signs when it reports a move, the operation's bytes, and whole
//! runs replayed event by event, refusals and replays included.

use std::collections::BTreeMap;

use arugula_e2e::run::{DeployApply, Event, Op, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::harness::{Ctx, Example, Spec, input};

pub const FILES: &[Spec] = &[
    Spec { module: "run", name: "state", run: run_state, make: states },
    Spec { module: "run", name: "event", run: run_event, make: events },
    Spec { module: "run", name: "op", run: run_op, make: ops },
    Spec { module: "run", name: "replay", run: run_replay, make: replays },
];

fn parse(s: &str) -> anyhow::Result<State> {
    State::parse(s).ok_or_else(|| anyhow::anyhow!("no state {s:?}"))
}

// ---- states ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateIn {
    state: String,
}

fn run_state(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: StateIn = input(v)?;
    let Some(s) = State::parse(&i.state) else { return Ok(json!({ "state": null })) };
    let to: Vec<&str> = State::ALL.into_iter().filter(|t| s.may_become(*t)).map(State::as_str).collect();
    Ok(json!({
        "state": s.as_str(),
        "terminal": s.terminal(),
        "daemon_only": s.daemon_only(),
        "control_only": s.control_only(),
        "may_become": to,
    }))
}

fn states() -> Example {
    let mut ex = Example::new(
        "run",
        "state",
        run_state,
        "A run's states (S37 q4, M210): pending, then approved (its authority signed) or straight to running \
         under a standing grant, then succeeded or failed; denied, canceled and expired are dead ends that never \
         ran. Nothing leaves a terminal state and nothing goes back to pending: a run asked again is a new run. \
         Only the run's daemon reports running and succeeded (it knows the process started, or ended well); only \
         control records pending, approved, denied and canceled, as the signed objects behind them arrive; \
         expired and failed may come from either.",
        &[
            "arugula_e2e::run::State::parse",
            "arugula_e2e::run::State::may_become",
            "arugula_e2e::run::State::terminal",
            "arugula_e2e::run::State::daemon_only",
            "arugula_e2e::run::State::control_only",
        ],
    )
    .shapes(
        "state: a state's name",
        "state: the name, or null for no such state; terminal, daemon_only, control_only; may_become: the states it \
         may move to, in the order pending, approved, denied, canceled, expired, running, succeeded, failed",
    );
    let table: [(&str, &[&str]); 8] = [
        ("pending", &["approved", "denied", "canceled", "expired", "running", "failed"]),
        ("approved", &["canceled", "expired", "running", "failed"]),
        ("denied", &[]),
        ("canceled", &[]),
        ("expired", &[]),
        ("running", &["succeeded", "failed"]),
        ("succeeded", &[]),
        ("failed", &[]),
    ];
    for (s, to) in table {
        let st = parse(s).unwrap();
        ex.case(
            &format!("a {s} run"),
            json!({ "state": s }),
            json!({ "state": s, "terminal": st.terminal(), "daemon_only": st.daemon_only(),
                    "control_only": st.control_only(), "may_become": to }),
        );
    }
    for s in ["done", "Pending", ""] {
        ex.case(&format!("{s:?}, no state"), json!({ "state": s }), json!({ "state": null }));
    }
    ex
}

// ---- the signed event ----

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct EventIn {
    run: String,
    seq: u64,
    from: Option<String>,
    to: String,
    at: u64,
    detail: String,
    daemon: String,
}

impl EventIn {
    fn with<T>(&self, f: impl FnOnce(&Event) -> T) -> anyhow::Result<T> {
        let from = self.from.as_deref().map(parse).transpose()?;
        let ev = Event {
            run: &self.run,
            seq: self.seq,
            from,
            to: parse(&self.to)?,
            at: self.at,
            detail: &self.detail,
            daemon: &self.daemon,
        };
        Ok(f(&ev))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventCheckIn {
    event: EventIn,
    sign: String,
    sig: String,
}

fn run_event(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: EventCheckIn = input(v)?;
    i.event.with(|e| json!({ "body": e.body(), "well_formed": e.well_formed(), "verified": e.verify(&i.sign, &i.sig) }))
}

fn event_json(run: &str, seq: u64, from: Option<&str>, to: &str, at: u64, detail: &str, daemon: &str) -> Value {
    json!({ "run": run, "seq": seq, "from": from, "to": to, "at": at, "detail": detail, "daemon": daemon })
}

fn sign_event(e: &Value, keys: &arugula_e2e::DeviceKeys) -> String {
    let e: EventIn = serde_json::from_value(e.clone()).unwrap();
    e.with(|ev| ev.sign(keys)).unwrap()
}

fn events() -> Example {
    let mut ex = Example::new(
        "run",
        "event",
        run_event,
        "The line a run's daemon signs when it reports a move (M210): the run, the event's number in the run \
         (from 1; so a signature is good for one place in one run's history, and can't be replayed onto another \
         run or later in the same one), the move (from - for a run's first event), when, the SHA-256 of the \
         event's detail exactly as stored (a JSON object's text), and the daemon. verified: well formed (the run \
         and daemon ids 1 to 128 bytes, no space or control character, so one event's body can't read as \
         another's) and the daemon's Ed25519 signature over the body.",
        &["arugula_e2e::run::Event::body", "arugula_e2e::run::Event::well_formed", "arugula_e2e::run::Event::verify"],
    )
    .shapes(
        "event: { run, seq, from (a state or null), to, at, detail, daemon }; sign: the daemon's Ed25519 key, hex; \
         sig: the signature, hex",
        "body: the signed text; well_formed; verified",
    );
    let keys = ex.key("daemon", 1);
    let other = ex.key("another-daemon", 2);
    let d = keys.id();
    let pk = hex::encode(keys.sign_public());
    let ev = event_json("run_1", 2, Some("pending"), "running", 10_000, "{}", &d);
    let sig = sign_event(&ev, &keys);
    let mut case = |about: &str, e: Value, sign: &str, s: &str, verified: bool| {
        let got = ex.computed(about, json!({ "event": e, "sign": sign, "sig": s }));
        assert_eq!(got["verified"], json!(verified), "{about}");
    };
    case("the daemon's report that the run started", ev.clone(), &pk, &sig, true);
    let changed = |k: &str, v: Value| {
        let mut e = ev.clone();
        e[k] = v;
        e
    };
    case("the same signature, another seq", changed("seq", json!(3)), &pk, &sig, false);
    case("another run", changed("run", json!("run_2")), &pk, &sig, false);
    case("from another state", changed("from", json!("approved")), &pk, &sig, false);
    case("to another state", changed("to", json!("failed")), &pk, &sig, false);
    case("another time", changed("at", json!(10_001)), &pk, &sig, false);
    case("another detail", changed("detail", json!("{\"exit\":0}")), &pk, &sig, false);
    case("the detail with a space more: its exact text is hashed", changed("detail", json!("{ }")), &pk, &sig, false);
    case("said to be another daemon's", changed("daemon", json!(other.id())), &pk, &sig, false);
    case("checked against another key", ev.clone(), &hex::encode(other.sign_public()), &sig, false);
    let first = event_json("run_1", 1, None, "pending", 9_000, "{}", &d);
    let s1 = sign_event(&first, &keys);
    case("a first event: from -", first, &pk, &s1, true);
    let done = event_json("run_1", 3, Some("running"), "succeeded", 20_000, "{\"exit\":0}", &d);
    let s3 = sign_event(&done, &keys);
    case("the run succeeded, exit 0", done, &pk, &s3, true);
    let bad = event_json("run_1\nseq 9", 2, Some("pending"), "running", 10_000, "{}", &d);
    let sb = sign_event(&bad, &keys);
    case("a newline in the run's id, signed: not well formed, so not verified", bad, &pk, &sb, false);
    let spaced = event_json("run 1", 2, Some("pending"), "running", 10_000, "{}", &d);
    let ss = sign_event(&spaced, &keys);
    case("a space in the run's id, signed", spaced, &pk, &ss, false);
    let long = event_json(&"r".repeat(129), 2, Some("pending"), "running", 10_000, "{}", &d);
    let sl = sign_event(&long, &keys);
    case("a run id of 129 bytes, signed", long, &pk, &sl, false);
    ex
}

// ---- the operation ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpJson {
    #[serde(default)]
    argv: Vec<String>,
    instruction: Option<String>,
    agent: Option<String>,
    directory: Option<String>,
    deploy: Option<DeployJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeployJson {
    deploy: String,
    driver: String,
    op: String,
}

fn op_json(o: &Op) -> Value {
    json!({
        "argv": o.argv,
        "instruction": o.instruction,
        "agent": o.agent,
        "directory": o.directory,
        "deploy": o.deploy.as_ref().map(|d| json!({ "deploy": d.deploy, "driver": d.driver, "op": d.op })),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum OpIn {
    Encode { op: OpJson, daemon: String },
    Decode { bytes: String },
}

fn run_op(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(match input::<OpIn>(v)? {
        OpIn::Encode { op, daemon } => {
            let o = Op {
                argv: op.argv,
                instruction: op.instruction,
                agent: op.agent,
                directory: op.directory,
                deploy: op.deploy.map(|d| DeployApply { deploy: d.deploy, driver: d.driver, op: d.op }),
            };
            json!({ "kind": o.kind(), "bytes": o.encode(&daemon) })
        }
        OpIn::Decode { bytes } => match Op::decode(&bytes) {
            Some((daemon, o)) => json!({ "daemon": daemon, "op": op_json(&o) }),
            None => json!({ "refused": true }),
        },
    })
}

fn ops() -> Example {
    let mut ex = Example::new(
        "run",
        "op",
        run_op,
        "What a run does (S37 q3): a command (kind run, its arguments base64, in order), an instruction to an \
         agent (kind agent, from a recipe or -), or a deploy's apply (kind deploy, M219: which deploy, its \
         driver and the hash of the operation its owner approved, and nothing else), for one daemon, in a \
         directory (base64) or -. Its bytes are what a run's hash, its standing grant and any approval name, so \
         the daemon reads them back only from exactly the bytes encode makes: an extra line, one out of place, \
         another version, bad base64, or a deploy with a command each refuse.",
        &["arugula_e2e::run::Op::encode", "arugula_e2e::run::Op::decode", "arugula_e2e::run::Op::kind"],
    )
    .shapes(
        "encode: { op: { argv, instruction, agent, directory, deploy: { deploy, driver, op } or null }, daemon }, or \
         decode: { bytes }",
        "encode: kind and bytes; decode: the daemon and the op, or refused",
    );
    let run =
        json!({ "argv": ["make", "it's"], "instruction": null, "agent": null, "directory": "/w", "deploy": null });
    let agent = json!({ "argv": [], "instruction": "fix it", "agent": "claude", "directory": null, "deploy": null });
    let bare_agent = json!({ "argv": [], "instruction": "fix it\nand say so", "agent": null, "directory": "/work/repo", "deploy": null });
    let plain = json!({ "argv": ["ls"], "instruction": null, "agent": null, "directory": null, "deploy": null });
    let dep = json!({ "argv": [], "instruction": null, "agent": null, "directory": null,
                      "deploy": { "deploy": "dep_1", "driver": "fake", "op": "ab".repeat(32) } });
    let mut bytes = BTreeMap::new();
    for (about, op) in [
        ("a command in a directory", &run),
        ("an instruction to a recipe's agent", &agent),
        ("an instruction to the default agent, with a newline, in a directory", &bare_agent),
        ("a command in the workspace's home", &plain),
        ("a deploy's apply", &dep),
    ] {
        let got = ex.computed(&format!("encode {about}"), json!({ "encode": { "op": op, "daemon": "d1" } }));
        bytes.insert(about, got["bytes"].as_str().unwrap().to_owned());
    }
    for (about, b) in &bytes {
        let got = ex.computed(&format!("decode {about}"), json!({ "decode": { "bytes": b } }));
        assert!(got.get("refused").is_none(), "{about}");
    }
    let cmd = &bytes["a command in a directory"];
    let swapped = cmd.replace("argv bWFrZQ==\nargv aXQncw==\n", "argv aXQncw==\nargv bWFrZQ==\n");
    let got = ex.computed(
        "decode the arguments in the other order: another command",
        json!({ "decode": { "bytes": swapped } }),
    );
    assert_eq!(got["op"]["argv"], json!(["it's", "make"]));
    let d = &bytes["a deploy's apply"];
    for (about, bad) in [
        ("the kind changed", cmd.replace("kind run", "kind agent")),
        ("a session named", cmd.replace("session -", "session s1")),
        ("a pane named", cmd.replace("pane - -", "pane 3 1")),
        ("a line more", format!("{cmd}env HOME\n")),
        ("no arguments", cmd.replace("argv bWFrZQ==\nargv aXQncw==\n", "")),
        ("no last newline", cmd.trim_end().to_owned()),
        ("version 2", cmd.replace("arugula op v1", "arugula op v2")),
        ("an argument that isn't base64", cmd.replace("bWFrZQ==", "!!")),
        (
            "the directory's line moved after the arguments",
            cmd.replace("cwd L3c=\nargv bWFrZQ==\nargv aXQncw==\n", "argv bWFrZQ==\nargv aXQncw==\ncwd L3c=\n"),
        ),
        ("no daemon", cmd.replace("daemon d1", "daemon ")),
        ("a deploy's kind changed to run", d.replace("kind deploy", "kind run")),
        ("a deploy in a directory", d.replace("cwd -", "cwd L3c=")),
        ("a deploy with a command", format!("{d}argv bWFrZQ==\n")),
        ("a deploy with no driver", d.replace("driver fake\n", "")),
        ("a deploy with no deploy", d.replace("deploy dep_1\n", "")),
        ("a deploy's hash after two spaces", d.replace("apply ", "apply  ")),
        ("nothing to do", "arugula op v1\ndaemon d1\nkind run\nsession -\npane - -\ncwd -\n".to_owned()),
    ] {
        ex.case(&format!("decode {about}"), json!({ "decode": { "bytes": bad } }), json!({ "refused": true }));
    }
    ex
}

// ---- replays ----

#[derive(Deserialize)]
#[serde(tag = "by", rename_all = "snake_case", deny_unknown_fields)]
enum Report {
    /// Control recorded it.
    Control { from: Option<String>, to: String },
    /// An approval of the run's operation arrived: it moves to approved.
    Approval { from: String },
    /// A daemon reported it, signed.
    Daemon { event: EventIn, sig: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayIn {
    run: String,
    daemon: String,
    grant: bool,
    keys: BTreeMap<String, String>,
    events: Vec<Report>,
}

const RULES: &str = "Each event, in order, is taken or refused; a refused one changes nothing. The first must be \
                     control's, from null to pending (not_made otherwise, and for anything before it). A daemon's \
                     report must have a from and verify (run/event) with the key the directory (keys) holds for \
                     the daemon it names (signature). Then: from must be the run's state now (moved); the move one \
                     S37 allows (disallowed); one its reporter may make (reporter): control denied, canceled, \
                     expired or failed, an approval approved, a daemon anything but a control_only state; a \
                     daemon's report must be about this run from this run's daemon (other_daemon), and its seq \
                     the run's next, every event before it counted, control's too (seq); pending straight to \
                     running needs the run's standing grant (no_grant). These are the platform's checks \
                     (crates/platform/src/db/runs.rs, move_run_in) over arugula_e2e::run, in its order; the \
                     detail's own rules there (an exit code, a reason) aren't here.";

fn run_replay(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: ReplayIn = input(v)?;
    let mut state: Option<State> = None;
    let mut seq = 0u64;
    let mut steps = vec![];
    for r in &i.events {
        let step = (|| -> anyhow::Result<Result<State, &'static str>> {
            let (from, to) = match r {
                Report::Control { from, to } => (from.as_deref().map(parse).transpose()?, parse(to)?),
                Report::Approval { from } => (Some(parse(from)?), State::Approved),
                Report::Daemon { event, sig } => {
                    let key = i.keys.get(&event.daemon);
                    let ok = event.with(|e| e.from.is_some() && key.is_some_and(|k| e.verify(k, sig)))?;
                    if !ok {
                        return Ok(Err("signature"));
                    }
                    (event.from.as_deref().map(parse).transpose()?, parse(&event.to)?)
                }
            };
            let Some(now) = state else {
                return Ok(match (r, from, to) {
                    (Report::Control { .. }, None, State::Pending) => Ok(State::Pending),
                    _ => Err("not_made"),
                });
            };
            if from != Some(now) {
                return Ok(Err("moved"));
            }
            if !now.may_become(to) {
                return Ok(Err("disallowed"));
            }
            let may = match r {
                Report::Control { .. } => {
                    matches!(to, State::Denied | State::Canceled | State::Expired | State::Failed)
                }
                Report::Approval { .. } => true,
                Report::Daemon { .. } => !to.control_only(),
            };
            if !may {
                return Ok(Err("reporter"));
            }
            if let Report::Daemon { event, .. } = r {
                if event.run != i.run || event.daemon != i.daemon {
                    return Ok(Err("other_daemon"));
                }
                if event.seq != seq + 1 {
                    return Ok(Err("seq"));
                }
            }
            if (now, to) == (State::Pending, State::Running) && !i.grant {
                return Ok(Err("no_grant"));
            }
            Ok(Ok(to))
        })()?;
        steps.push(match step {
            Ok(to) => {
                state = Some(to);
                seq += 1;
                json!({ "took": to.as_str() })
            }
            Err(why) => json!({ "refused": why }),
        });
    }
    Ok(json!({ "steps": steps, "state": state.map(State::as_str) }))
}

fn replays() -> Example {
    let mut ex = Example::new(
        "run",
        "replay",
        run_replay,
        &format!("Whole runs, replayed event by event: who may move a run, when, and with what. {RULES}"),
        &[
            "arugula_e2e::run::State::may_become",
            "arugula_e2e::run::State::control_only",
            "arugula_e2e::run::Event::verify",
        ],
    )
    .shapes(
        "run: its id; daemon: its daemon's id (as its op names it); grant: whether a standing grant covers it; keys: \
         the hub's directory, { daemon id: Ed25519 key, hex }; events: each { by: control, from, to }, { by: \
         approval, from } or { by: daemon, event: { run, seq, from, to, at, detail, daemon }, sig }",
        "steps: each event's { took: the state after } or { refused: why }; state: the run's state at the end",
    );
    let me = ex.key("daemon", 1);
    let other = ex.key("another-daemon", 2);
    let stranger = ex.key("not-in-the-directory", 3);
    let d = me.id();
    let keys = json!({ d.clone(): hex::encode(me.sign_public()), other.id(): hex::encode(other.sign_public()) });
    let made = json!({ "by": "control", "from": null, "to": "pending" });
    let control = |from: &str, to: &str| json!({ "by": "control", "from": from, "to": to });
    let approval = |from: &str| json!({ "by": "approval", "from": from });
    let daemon = |k: &arugula_e2e::DeviceKeys, run: &str, seq: u64, from: &str, to: &str, detail: &str| {
        let e = event_json(run, seq, Some(from), to, 10_000 + seq, detail, &k.id());
        let sig = sign_event(&e, k);
        json!({ "by": "daemon", "event": e, "sig": sig })
    };
    let mut case = |about: &str, grant: bool, evs: Vec<Value>, steps: &[&str], end: Option<&str>| {
        let input = json!({ "run": "run_1", "daemon": d, "grant": grant, "keys": keys, "events": evs });
        let want: Vec<Value> = steps
            .iter()
            .map(|s| match s.strip_prefix('!') {
                Some(why) => json!({ "refused": why }),
                None => json!({ "took": s }),
            })
            .collect();
        ex.case(about, input, json!({ "steps": want, "state": end }));
    };
    let r = "run_1";
    let running = daemon(&me, r, 3, "approved", "running", "{}");
    let done = daemon(&me, r, 4, "running", "succeeded", "{\"exit\":0}");
    case(
        "approved, run and succeeded",
        false,
        vec![made.clone(), approval("pending"), running.clone(), done.clone()],
        &["pending", "approved", "running", "succeeded"],
        Some("succeeded"),
    );
    case(
        "failed with an exit code",
        false,
        vec![
            made.clone(),
            approval("pending"),
            running.clone(),
            daemon(&me, r, 4, "running", "failed", "{\"exit\":2}"),
        ],
        &["pending", "approved", "running", "failed"],
        Some("failed"),
    );
    let granted = daemon(&me, r, 2, "pending", "running", "{}");
    case(
        "a standing grant: straight from pending to running",
        true,
        vec![made.clone(), granted.clone(), daemon(&me, r, 3, "running", "succeeded", "{\"exit\":0}")],
        &["pending", "running", "succeeded"],
        Some("succeeded"),
    );
    case(
        "straight to running without a grant",
        false,
        vec![made.clone(), granted.clone()],
        &["pending", "!no_grant"],
        Some("pending"),
    );
    case(
        "the running report sent again",
        false,
        vec![made.clone(), approval("pending"), running.clone(), running.clone()],
        &["pending", "approved", "running", "!moved"],
        Some("running"),
    );
    case(
        "a report signed for a later place in the run",
        false,
        vec![made.clone(), approval("pending"), daemon(&me, r, 5, "approved", "running", "{}")],
        &["pending", "approved", "!seq"],
        Some("approved"),
    );
    case(
        "a report signed for an earlier place: before the approval was counted",
        false,
        vec![made.clone(), approval("pending"), daemon(&me, r, 2, "approved", "running", "{}")],
        &["pending", "approved", "!seq"],
        Some("approved"),
    );
    case(
        "another run's report, replayed onto this one",
        false,
        vec![made.clone(), approval("pending"), daemon(&me, "run_2", 3, "approved", "running", "{}")],
        &["pending", "approved", "!other_daemon"],
        Some("approved"),
    );
    case(
        "another daemon in the directory reports it",
        false,
        vec![made.clone(), approval("pending"), daemon(&other, r, 3, "approved", "running", "{}")],
        &["pending", "approved", "!other_daemon"],
        Some("approved"),
    );
    case(
        "a daemon the directory doesn't know reports it",
        false,
        vec![made.clone(), approval("pending"), daemon(&stranger, r, 3, "approved", "running", "{}")],
        &["pending", "approved", "!signature"],
        Some("approved"),
    );
    let mut tampered = running.clone();
    tampered["event"]["detail"] = json!("{\"exit\":0}");
    case(
        "the report's detail changed after it was signed",
        false,
        vec![made.clone(), approval("pending"), tampered],
        &["pending", "approved", "!signature"],
        Some("approved"),
    );
    let mut first = json!({ "by": "daemon" });
    let e = event_json(r, 1, None, "pending", 10_000, "{}", &d);
    first["sig"] = json!(sign_event(&e, &me));
    first["event"] = e;
    case(
        "a daemon's signed first event: a run's first is control's",
        false,
        vec![first, made.clone()],
        &["!signature", "pending"],
        Some("pending"),
    );
    case(
        "a move before the run was made",
        false,
        vec![approval("pending"), made.clone()],
        &["!not_made", "pending"],
        Some("pending"),
    );
    case("a second first event", false, vec![made.clone(), made.clone()], &["pending", "!moved"], Some("pending"));
    case(
        "the daemon reports it denied",
        false,
        vec![made.clone(), daemon(&me, r, 2, "pending", "denied", "{}")],
        &["pending", "!reporter"],
        Some("pending"),
    );
    case(
        "the daemon reports it approved",
        false,
        vec![made.clone(), daemon(&me, r, 2, "pending", "approved", "{}")],
        &["pending", "!reporter"],
        Some("pending"),
    );
    case(
        "the daemon reports it canceled",
        false,
        vec![made.clone(), approval("pending"), daemon(&me, r, 3, "approved", "canceled", "{}")],
        &["pending", "approved", "!reporter"],
        Some("approved"),
    );
    case(
        "control says it's running",
        false,
        vec![made.clone(), approval("pending"), control("approved", "running")],
        &["pending", "approved", "!reporter"],
        Some("approved"),
    );
    case(
        "control says it succeeded",
        false,
        vec![made.clone(), approval("pending"), running.clone(), control("running", "succeeded")],
        &["pending", "approved", "running", "!reporter"],
        Some("running"),
    );
    case(
        "control approves it without an approval",
        false,
        vec![made.clone(), control("pending", "approved")],
        &["pending", "!reporter"],
        Some("pending"),
    );
    case(
        "control fails a running run (its dispatch was lost)",
        false,
        vec![made.clone(), approval("pending"), running.clone(), control("running", "failed")],
        &["pending", "approved", "running", "failed"],
        Some("failed"),
    );
    case("denied", false, vec![made.clone(), control("pending", "denied")], &["pending", "denied"], Some("denied"));
    case(
        "canceled before it started, then the daemon reports it running",
        false,
        vec![made.clone(), approval("pending"), control("approved", "canceled"), running.clone()],
        &["pending", "approved", "canceled", "!moved"],
        Some("canceled"),
    );
    case(
        "canceled, then the daemon reports from canceled",
        false,
        vec![
            made.clone(),
            approval("pending"),
            control("approved", "canceled"),
            daemon(&me, r, 4, "canceled", "running", "{}"),
        ],
        &["pending", "approved", "canceled", "!disallowed"],
        Some("canceled"),
    );
    case(
        "expired by control's clock, then approved",
        false,
        vec![made.clone(), control("pending", "expired"), approval("expired")],
        &["pending", "expired", "!disallowed"],
        Some("expired"),
    );
    case(
        "expired by the daemon's clock",
        false,
        vec![made.clone(), approval("pending"), daemon(&me, r, 3, "approved", "expired", "{}")],
        &["pending", "approved", "expired"],
        Some("expired"),
    );
    case(
        "the daemon's check at the start failed",
        false,
        vec![
            made.clone(),
            approval("pending"),
            daemon(&me, r, 3, "approved", "failed", "{\"why\":\"approval_expired\"}"),
        ],
        &["pending", "approved", "failed"],
        Some("failed"),
    );
    case(
        "succeeded, then canceled",
        false,
        vec![made.clone(), approval("pending"), running.clone(), done.clone(), control("succeeded", "canceled")],
        &["pending", "approved", "running", "succeeded", "!disallowed"],
        Some("succeeded"),
    );
    case(
        "approved twice",
        false,
        vec![made.clone(), approval("pending"), approval("pending")],
        &["pending", "approved", "!moved"],
        Some("approved"),
    );
    case(
        "succeeded straight from approved",
        false,
        vec![made.clone(), approval("pending"), daemon(&me, r, 3, "approved", "succeeded", "{\"exit\":0}")],
        &["pending", "approved", "!disallowed"],
        Some("approved"),
    );
    case(
        "a refused report doesn't take a place: the next one is still 3",
        false,
        vec![made.clone(), approval("pending"), daemon(&me, r, 3, "approved", "canceled", "{}"), running.clone()],
        &["pending", "approved", "!reporter", "running"],
        Some("running"),
    );
    ex
}
