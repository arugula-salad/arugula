//! `deploy`: what a deploy's owner approves (the operation's bytes and
//! hash, the approval's bytes and passkey challenge, v2 and v1), what a
//! machine checks before it applies one (`check_apply`: the owner's passkey
//! against the passkey log it pinned), which secrets a driver takes, and
//! planter's and apps-platform's names and sources.

use std::collections::BTreeMap;

use arugula_e2e::{
    Kind,
    deploy::{
        ApplyRefusal, Approval, Decision, Operation, Pinned, apps_platform, check_apply, driver_secrets, driver_takes,
        planter, secret_name_ok,
    },
    passkey::{Assertion, Change, Draft, EDDSA, Log, NO_AAGUID, Origins},
    team::TeamRole,
};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
    harness::{Ctx, Example, OWNERS_SHAPE, OwnersIn, Spec, cert, input, member, person},
    team::roster,
};

pub const FILES: &[Spec] = &[
    Spec { module: "deploy", name: "operation", run: run_operation, make: operation },
    Spec { module: "deploy", name: "approval", run: run_approval, make: approval },
    Spec { module: "deploy", name: "check_apply", run: run_check_apply, make: check_apply_cases },
    Spec { module: "deploy", name: "secrets", run: run_secrets, make: secrets },
    Spec { module: "deploy", name: "planter", run: run_planter, make: planter_names },
    Spec { module: "deploy", name: "apps_platform", run: run_apps_platform, make: apps_platform_names },
];

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn op(plan: &str, name: &str) -> Operation {
    Operation {
        project: "prj_1".into(),
        environment: "env_1".into(),
        environment_name: name.into(),
        plan: plan.into(),
        source_kind: "commit".into(),
        source_ref: SHA.into(),
        source_where: "acme/app".into(),
        planned: "ab".repeat(32),
    }
}

/// A plan's digest, as made up for these cases (drivers/ has real ones).
fn plan(n: char) -> String {
    n.to_string().repeat(64)
}

// ---- the operation ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpIn {
    op: String,
}

fn run_operation(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: OpIn = input(v)?;
    let o: Operation = ctx.get(&i.op)?;
    Ok(json!({ "body": o.body(), "hash": o.hash() }))
}

fn operation() -> Example {
    let mut ex = Example::new(
        "deploy",
        "operation",
        run_operation,
        "What a deploy's owner approves (M218): the project, the environment (its id and its name, so the name a \
         person reads is in the hashed bytes), the plan's SHA-256, the source, and the hash of the deploy's \
         planned event in its environment's log, as a fixed text. Its hash is SHA-256 over exactly that text, \
         hex: change any byte (another plan, commit, environment or log) and the approval no longer names it.",
        &["arugula_e2e::deploy::Operation::body", "arugula_e2e::deploy::Operation::hash"],
    )
    .shapes("op: an operation's label", "body: the hashed text; hash: its SHA-256, hex");
    let base = op(&plan('a'), "production");
    let objects = [
        ("commit", "a commit, to production", base.clone()),
        (
            "artifact",
            "an artifact, by digest, from the platform's store",
            Operation {
                source_kind: "artifact".into(),
                source_ref: format!("sha256:{}", "c".repeat(64)),
                source_where: "platform".into(),
                ..base.clone()
            },
        ),
        ("renamed", "the same, its environment renamed", Operation { environment_name: "prod".into(), ..base.clone() }),
        ("other-plan", "the same, another plan", Operation { plan: plan('b'), ..base.clone() }),
        ("other-log", "the same, another planned event", Operation { planned: "cd".repeat(32), ..base.clone() }),
    ];
    for (l, about, o) in objects {
        ex.object(l, &o);
        ex.computed(about, json!({ "op": l }));
    }
    ex
}

// ---- the approval ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalIn {
    approval: String,
    sign: String,
}

fn run_approval(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: ApprovalIn = input(v)?;
    let a: Approval = ctx.get(&i.approval)?;
    Ok(json!({
        "body": a.body(),
        "well_formed": a.well_formed(),
        "signed_by": a.signed_by(&i.sign),
        "passkey_challenge": a.passkey_challenge(),
    }))
}

fn approval() -> Example {
    let mut ex = Example::new(
        "deploy",
        "approval",
        run_approval,
        "A deploy's approval, or its deny, as a fixed text: the deploy, the operation's hash, the approver's \
         account, the decision (once or deny) and when the apply must start by (ms). v2 (DK4, aru-126; `v: 2`) \
         is the only kind made now: no device signs it (no by, no sig), and its signature is a passkey's \
         assertion whose WebAuthn challenge is SHA-256 of the body, base64url without padding. v1 (M218; no \
         `v`) named the signing device too, as its body's last line, and carried that device's Ed25519 \
         signature over the body (sig). well_formed: no field is empty, longer than 200 bytes, or holds \
         anything but printable ASCII without spaces, so the body reads one way only; a v2 has no by and no \
         sig, and no other version is. signed_by: a v1, well formed, and the signature is the given key's; \
         never a v2.",
        &[
            "arugula_e2e::deploy::Approval::body",
            "arugula_e2e::deploy::Approval::well_formed",
            "arugula_e2e::deploy::Approval::signed_by",
            "arugula_e2e::deploy::Approval::passkey_challenge",
        ],
    )
    .shapes(
        "approval: an approval's label; sign: an Ed25519 public key, hex, to check a v1's signature against",
        "body; well_formed; signed_by: whether sign signed it; passkey_challenge",
    );
    let alice = person(&mut ex, "alice", 1);
    let other = ex.key("someone-else", 2);
    let o = op(&plan('a'), "production");
    ex.object("op", &o);
    let v2 = Approval::v2("dep_1", &o, "alice", Decision::Once, 90_000);
    let v2_deny = Approval::v2("dep_1", &o, "alice", Decision::Deny, 90_000);
    let v2_named = Approval { by: alice.keys.id(), ..v2.clone() };
    let v2_signed = Approval { sig: hex::encode(alice.keys.signature(v2.body().as_bytes())), ..v2.clone() };
    let v3 = Approval { v: 3, ..v2.clone() };
    let once = Approval::v1(&alice.keys, "dep_1", &o, "alice", Decision::Once, 90_000);
    let deny = Approval::v1(&alice.keys, "dep_1", &o, "alice", Decision::Deny, 90_000);
    let pk = hex::encode(alice.keys.sign_public());
    let them = hex::encode(other.sign_public());
    let mut later = once.clone();
    later.expires = 99_000;
    let mut spaced = Approval::v1(&alice.keys, "dep_1 dep_2", &o, "alice", Decision::Once, 90_000);
    spaced.account = "alice".into();
    let mut moved = once.clone();
    moved.deploy = "dep_2".into();
    for (l, a) in [
        ("v2", &v2),
        ("v2-deny", &v2_deny),
        ("v2-naming-a-device", &v2_named),
        ("v2-with-a-signature", &v2_signed),
        ("v3", &v3),
        ("once", &once),
        ("deny", &deny),
        ("expires-changed", &later),
        ("space-in-deploy", &spaced),
        ("other-deploy", &moved),
    ] {
        ex.object(l, a);
    }
    let mut case = |about: &str, l: &str, sign: &str, ok: bool, form: bool| {
        let got = ex.computed(about, json!({ "approval": l, "sign": sign }));
        assert_eq!((got["signed_by"].as_bool(), got["well_formed"].as_bool()), (Some(ok), Some(form)), "{about}");
    };
    case("alice approves the deploy once, v2: her passkey's assertion is its signature", "v2", &pk, false, true);
    case("alice denies it, v2", "v2-deny", &pk, false, true);
    case("a v2 naming a device: not well formed", "v2-naming-a-device", &pk, false, false);
    case("a v2 carrying a device's signature: not well formed", "v2-with-a-signature", &pk, false, false);
    case("version 3: not one", "v3", &pk, false, false);
    case("v1: alice approves the deploy once, from her device", "once", &pk, true, true);
    case("v1: alice denies it: a deny is signed too", "deny", &pk, true, true);
    case("v1, checked against another key", "once", &them, false, true);
    case("v1, its expiry pushed out after she signed", "expires-changed", &pk, false, true);
    case("v1, moved to another deploy after she signed", "other-deploy", &pk, false, true);
    case(
        "v1, a space in a field: not well formed, so never signed_by, whoever signed it",
        "space-in-deploy",
        &pk,
        false,
        false,
    );
    ex
}

// ---- check_apply ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyIn {
    approval: String,
    op: String,
    deploy: String,
    owners: OwnersIn,
    /// The passkey logs the machine pinned: account → a log's label.
    passkey_logs: BTreeMap<String, String>,
    origins: Origins,
    /// An assertion's label, or none.
    passkey: Option<String>,
    /// The log the work carried: a log's label, or none.
    carried: Option<String>,
    now: u64,
}

fn run_check_apply(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: ApplyIn = input(v)?;
    let owners = i.owners.load(ctx)?;
    let owners = owners.owners();
    let logs: BTreeMap<String, Log> =
        i.passkey_logs.iter().map(|(a, l)| Ok((a.clone(), ctx.get(l)?))).collect::<anyhow::Result<_>>()?;
    let passkey: Option<Assertion> = i.passkey.as_deref().map(|l| ctx.get(l)).transpose()?;
    let carried: Option<Log> = i.carried.as_deref().map(|l| ctx.get(l)).transpose()?;
    let pinned = Pinned { owners: &owners, passkey_logs: &logs, origins: &i.origins };
    let r = check_apply(
        &ctx.get(&i.approval)?,
        &ctx.get(&i.op)?,
        &i.deploy,
        &pinned,
        passkey.as_ref(),
        carried.as_ref(),
        i.now,
    );
    Ok(match r {
        Ok(()) => json!({ "ok": true }),
        Err(ApplyRefusal::Passkey(Some(why))) => json!({ "refused": "approval_passkey", "passkey": why.code() }),
        Err(e) => json!({ "refused": e.code() }),
    })
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

const RP: &str = "control.example.invalid";
const CONTROL: &str = "https://control.example.invalid";
const APP: &str = "https://app.example.invalid";

/// A test passkey: Ed25519 from seed byte `n`, its id `n` sixteen times.
struct Pk {
    key: ed25519_dalek::SigningKey,
    id: String,
}

impl Pk {
    fn new(n: u8) -> Self {
        Pk { key: ed25519_dalek::SigningKey::from_bytes(&[n; 32]), id: b64(&[n; 16]) }
    }

    /// Its `added` event in `account`'s log at `at`, after the log's last.
    fn add_to(&self, log: &mut Log, at: u64) {
        let e = Draft {
            account: log.account.clone(),
            event: Change::Added,
            credential: self.id.clone(),
            rp: RP.into(),
            alg: EDDSA,
            key: self.key.verifying_key().to_bytes().to_vec(),
            aaguid: NO_AAGUID.into(),
            at,
            by: "signup".into(),
        }
        .after(log.events.last());
        log.events.push(e);
    }

    /// An assertion of `kind` over `challenge` from `origin` for RP `rp`,
    /// with `flags`.
    fn sign(&self, kind: &str, challenge: &str, origin: &str, rp: &str, flags: u8) -> Assertion {
        use ed25519_dalek::Signer as _;
        let cd = json!({ "type": kind, "challenge": challenge, "origin": origin }).to_string();
        let mut data = Sha256::digest(rp.as_bytes()).to_vec();
        data.push(flags);
        data.extend([0, 0, 0, 0]);
        let mut msg = data.clone();
        msg.extend_from_slice(&Sha256::digest(cd.as_bytes()));
        Assertion {
            id: self.id.clone(),
            client_data_json: b64(cd.as_bytes()),
            authenticator_data: b64(&data),
            signature: b64(&self.key.sign(&msg).to_bytes()),
        }
    }

    fn over(&self, a: &Approval) -> Assertion {
        self.sign("webauthn.get", &a.passkey_challenge(), CONTROL, RP, 0x05)
    }
}

fn check_apply_cases() -> Example {
    let mut ex = Example::new(
        "deploy",
        "check_apply",
        run_check_apply,
        "What a machine checks before it applies a deploy (M219; DK4, aru-126), from what it pinned rather than \
         the platform's word, in this order, the first failure being the refusal: the approval is well formed \
         (approval_form; a v2, or a v1 made before DK4); for this deploy (approval_deploy); over exactly this \
         operation's hash (approval_operation); once, not a deny (approval_denied); not expired, expires > now \
         (approval_expired); its account an owner of the project, as the machine knows them, its team's roster \
         or its own account (approval_not_an_owner); a passkey assertion with it (approval_passkey, with no \
         `passkey`); the account's passkey log pinned (approval_passkey_log), and the assertion checked against \
         the pinned log at now, as passkey/verify does (approval_passkey, `passkey` saying which check failed: \
         not_live for a key not in the log or removed by now, origin for an origin not pinned for its RP, \
         challenge for other bytes, user without UV); then the log the work carried, if any (the platform's \
         word): it must be the pinned one, older, or one after it, its events the pinned one's first \
         (approval_passkey_log), and only its removals after the pinned log, at or before now, count \
         (approval_passkey, not_live, for the key removed): a key it adds is never taken, since an added event \
         by sign-up or GitHub needs no proof. No device plays a \
         part: which devices the owner has, and who certified them, is never looked at. A v1 is checked by its \
         passkey alone, as a v2 is.",
        &[
            "arugula_e2e::deploy::check_apply",
            "arugula_e2e::deploy::Approval::passkey_checks",
            "arugula_e2e::passkey::removed_since",
            "arugula_e2e::passkey::extends",
        ],
    )
    .shapes(
        &format!(
            "approval and op: labels; deploy: the deploy the run applies; {OWNERS_SHAPE}; passkey_logs: the \
             passkey logs the machine pinned, {{ account: a log's label }}; origins: which origins may use each \
             RP's passkeys, as the machine pinned them ({{ rp: [origins] }}); passkey: an assertion's label, or \
             null; carried: the label of the log the work carried, or null; now: the machine's clock, ms"
        ),
        "ok: true, or refused: the check's code (and passkey: passkey/verify's refusal)",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let ak = &alice.keys;
    // alice's only other device is a CLI: it plays no part.
    let cli = ex.key("alice-cli", 10);
    ex.object("alice-cli", &cert(&cli, "alice", Kind::Cli, "terminal", 1_100, ak));
    ex.object(
        "team-roster",
        &roster(1, 1_000, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Editor)], ak),
    );
    let account =
        json!({ "account": { "account": "alice", "root": alice.cert.device, "certs": ["alice", "alice-cli"] } });
    let team = json!({ "team": { "roster": "team-roster", "certs": {
        "alice": { "certs": ["alice", "alice-cli"] }, "bob": { "certs": ["bob"] } } } });

    // alice's passkeys: k1 added at 1000, k2 at 2000, k1 removed at 30000
    // (by a reset); bob's kb. k9 is in no log.
    let (k1, k2, kb, k9) = (Pk::new(0x31), Pk::new(0x32), Pk::new(0x33), Pk::new(0x39));
    let mut log = Log { account: "alice".into(), ..Default::default() };
    k1.add_to(&mut log, 1_000);
    k2.add_to(&mut log, 2_000);
    let before_removal = log.clone();
    let removal = log.events[0].removal(30_000, "reset").placed_after(log.events.last());
    log.events.push(removal);
    let mut rewritten = before_removal.clone();
    Pk::new(0x34).add_to(&mut rewritten, 30_000);
    // The platform's own key, appended `by signup` to a log it carries.
    let mut appended = log.clone();
    let k5 = Pk::new(0x35);
    k5.add_to(&mut appended, 40_000);
    let mut blog = Log { account: "bob".into(), ..Default::default() };
    kb.add_to(&mut blog, 1_000);
    for (l, g) in [
        ("alice-log", &log),
        ("alice-log-before-removal", &before_removal),
        ("alice-log-rewritten", &rewritten),
        ("alice-log-with-a-key-appended", &appended),
        ("bob-log", &blog),
    ] {
        ex.object(l, g);
    }
    let logs = json!({ "alice": "alice-log", "bob": "bob-log" });
    let origins = json!({ RP: [CONTROL] });

    let o = op(&plan('a'), "production");
    ex.object("op", &o);
    ex.object("op-other-plan", &op(&plan('b'), "production"));
    ex.object("op-renamed", &op(&plan('a'), "prod"));
    ex.object("op-other-project", &Operation { project: "prj_2".into(), ..o.clone() });
    let now = 50_000;
    let v2 = |ex: &mut Example, l: &str, account: &str, d: Decision, exp: u64| {
        let a = Approval::v2("dep_1", &o, account, d, exp);
        ex.object(l, &a);
        a
    };
    let good = v2(&mut ex, "by-alice", "alice", Decision::Once, 90_000);
    let assertion = |ex: &mut Example, l: &str, a: Assertion| -> Value {
        ex.object(l, &a);
        json!(l)
    };
    let p = assertion(&mut ex, "k2-over-by-alice", k2.over(&good));
    let mut cases: Vec<(String, String, Value, Value, Value, Value)> = vec![];
    let mut add = |about: &str, a: &str, owners: &Value, passkey: &Value, more: Value, want: Value| {
        cases.push((about.into(), a.into(), owners.clone(), passkey.clone(), more, want));
    };
    let ok = json!({ "ok": true });
    let no = |code: &str| json!({ "refused": code });
    let pk_no = |why: &str| json!({ "refused": "approval_passkey", "passkey": why });
    add(
        "alice, owner of her account's project, approves with her passkey",
        "by-alice",
        &account,
        &p,
        json!({}),
        ok.clone(),
    );
    add("alice, owner of the team's project, approves", "by-alice", &team, &p, json!({}), ok.clone());
    add(
        "the work carried the log the machine pinned",
        "by-alice",
        &team,
        &p,
        json!({ "carried": "alice-log" }),
        ok.clone(),
    );
    add("checked for another deploy", "by-alice", &account, &p, json!({ "deploy": "dep_2" }), no("approval_deploy"));
    add(
        "checked against another plan",
        "by-alice",
        &account,
        &p,
        json!({ "op": "op-other-plan" }),
        no("approval_operation"),
    );
    add(
        "checked against the environment renamed",
        "by-alice",
        &account,
        &p,
        json!({ "op": "op-renamed" }),
        no("approval_operation"),
    );
    add(
        "checked against another project's operation",
        "by-alice",
        &account,
        &p,
        json!({ "op": "op-other-project" }),
        no("approval_operation"),
    );
    let deny = v2(&mut ex, "deny-by-alice", "alice", Decision::Deny, 90_000);
    let pd = assertion(&mut ex, "k2-over-deny-by-alice", k2.over(&deny));
    add("a deny", "deny-by-alice", &account, &pd, json!({}), no("approval_denied"));
    add("its expiry is now", "by-alice", &account, &p, json!({ "now": 90_000 }), no("approval_expired"));
    add("1 ms before its expiry", "by-alice", &account, &p, json!({ "now": 89_999 }), ok.clone());
    let by_bob = v2(&mut ex, "by-bob", "bob", Decision::Once, 90_000);
    let pb = assertion(&mut ex, "kb-over-by-bob", kb.over(&by_bob));
    add("bob, an editor of the team, approves", "by-bob", &team, &pb, json!({}), no("approval_not_an_owner"));
    add("bob approves alice's own project", "by-bob", &account, &pb, json!({}), no("approval_not_an_owner"));
    let as_alice = assertion(&mut ex, "kb-over-by-alice", kb.over(&good));
    add("bob's passkey says it's alice's approval", "by-alice", &team, &as_alice, json!({}), pk_no("not_live"));
    add("no passkey assertion", "by-alice", &account, &Value::Null, json!({}), no("approval_passkey"));
    let p9 = assertion(&mut ex, "k9-over-by-alice", k9.over(&good));
    add("a passkey that's in no log", "by-alice", &account, &p9, json!({}), pk_no("not_live"));
    let p1 = assertion(&mut ex, "k1-over-by-alice", k1.over(&good));
    add("a passkey removed from her log before now", "by-alice", &account, &p1, json!({}), pk_no("not_live"));
    add(
        "the same, the machine having pinned her log before the removal",
        "by-alice",
        &account,
        &p1,
        json!({ "passkey_logs": { "alice": "alice-log-before-removal" } }),
        ok.clone(),
    );
    add(
        "the same, the work carrying the log since the removal",
        "by-alice",
        &account,
        &p1,
        json!({ "passkey_logs": { "alice": "alice-log-before-removal" }, "carried": "alice-log" }),
        pk_no("not_live"),
    );
    add(
        "the work carrying her log from before the removal the machine pinned: the pinned one counts",
        "by-alice",
        &account,
        &p1,
        json!({ "carried": "alice-log-before-removal" }),
        pk_no("not_live"),
    );
    add(
        "the work carrying a log that dropped the removal and added a key",
        "by-alice",
        &account,
        &p,
        json!({ "carried": "alice-log-rewritten" }),
        no("approval_passkey_log"),
    );
    let p5 = assertion(&mut ex, "k5-over-by-alice", k5.over(&good));
    add(
        "a key only the carried log has, appended by sign-up (the platform's own): never taken",
        "by-alice",
        &account,
        &p5,
        json!({ "carried": "alice-log-with-a-key-appended" }),
        pk_no("not_live"),
    );
    add(
        "her pinned key, the carried log having a key appended: fine",
        "by-alice",
        &account,
        &p,
        json!({ "carried": "alice-log-with-a-key-appended" }),
        ok.clone(),
    );
    add(
        "the machine pinned no log for her",
        "by-alice",
        &account,
        &p,
        json!({ "passkey_logs": { "bob": "bob-log" }, "carried": "alice-log" }),
        no("approval_passkey_log"),
    );
    let ch = good.passkey_challenge();
    let from_app = assertion(&mut ex, "k2-over-by-alice-from-app", k2.sign("webauthn.get", &ch, APP, RP, 0x05));
    add("from the platform page's origin, not pinned", "by-alice", &account, &from_app, json!({}), pk_no("origin"));
    add(
        "from the platform page's origin, pinned for the RP",
        "by-alice",
        &account,
        &from_app,
        json!({ "origins": { RP: [CONTROL, APP] } }),
        ok.clone(),
    );
    let other = Approval::v2("dep_9", &o, "alice", Decision::Once, 90_000).passkey_challenge();
    for (l, about, a, want) in [
        (
            "k2-create",
            "a passkey made for a registration (webauthn.create)",
            k2.sign("webauthn.create", &ch, CONTROL, RP, 0x05),
            pk_no("ceremony"),
        ),
        (
            "k2-other-challenge",
            "a passkey over another approval's challenge",
            k2.sign("webauthn.get", &other, CONTROL, RP, 0x05),
            pk_no("challenge"),
        ),
        ("k2-up-only", "user presence only", k2.sign("webauthn.get", &ch, CONTROL, RP, 0x01), pk_no("user")),
        ("k2-uv-only", "user verification only", k2.sign("webauthn.get", &ch, CONTROL, RP, 0x04), pk_no("user")),
        ("k2-more-flags", "more flags set: fine", k2.sign("webauthn.get", &ch, CONTROL, RP, 0x45), ok.clone()),
        ("k2-other-rp", "for another RP", k2.sign("webauthn.get", &ch, CONTROL, "other.invalid", 0x05), pk_no("rp")),
    ] {
        let v = assertion(&mut ex, l, a);
        add(about, "by-alice", &account, &v, json!({}), want);
    }
    let mut forged = k2.over(&good);
    forged.signature = b64(&[0u8; 64]);
    let forged = assertion(&mut ex, "k2-forged", forged);
    add("a signature that isn't the passkey's", "by-alice", &account, &forged, json!({}), pk_no("signature"));
    // A v1 from before DK4, by alice's CLI, applies by its passkey alone.
    let v1 = Approval::v1(&cli, "dep_1", &o, "alice", Decision::Once, 90_000);
    ex.object("v1-by-alice-cli", &v1);
    let pv1 = assertion(&mut ex, "k2-over-v1-by-alice-cli", k2.over(&v1));
    add(
        "a v1 made before DK4, by her CLI: its passkey alone counts",
        "v1-by-alice-cli",
        &account,
        &pv1,
        json!({}),
        ok.clone(),
    );
    add("a v1 with a v2's assertion", "v1-by-alice-cli", &account, &p, json!({}), pk_no("challenge"));
    add("a v2 with a v1's assertion", "by-alice", &account, &pv1, json!({}), pk_no("challenge"));
    let mut named = good.clone();
    named.by = cli.id();
    ex.object("v2-naming-a-device", &named);
    add("a v2 naming a device", "v2-naming-a-device", &account, &p, json!({}), no("approval_form"));
    let mut spaced = good.clone();
    spaced.deploy = "dep_1 dep_2".into();
    ex.object("space-in-deploy", &spaced);
    add(
        "a space in its deploy",
        "space-in-deploy",
        &account,
        &p,
        json!({ "deploy": "dep_1 dep_2" }),
        no("approval_form"),
    );
    for (about, a, owners, passkey, more, want) in cases {
        let mut input = json!({ "approval": a, "op": "op", "deploy": "dep_1", "owners": owners,
                                "passkey_logs": logs, "origins": origins, "passkey": passkey, "carried": null,
                                "now": now });
        for (k, v) in more.as_object().unwrap() {
            input[k] = v.clone();
        }
        ex.case(&about, input, want);
    }
    ex
}

// ---- secrets ----

#[derive(Deserialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
enum SecretsIn {
    DriverSecrets { driver: String },
    SecretNameOk { name: String },
    DriverTakes { driver: String, name: String },
}

fn run_secrets(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(match input::<SecretsIn>(v)? {
        SecretsIn::DriverSecrets { driver } => json!({ "secrets": driver_secrets(&driver) }),
        SecretsIn::SecretNameOk { name } => json!({ "ok": secret_name_ok(&name) }),
        SecretsIn::DriverTakes { driver, name } => json!({ "ok": driver_takes(&driver, &name) }),
    })
}

pub const DRIVERS: [&str; 8] = [
    "fake",
    "chant-fly",
    "apps-platform",
    "arugula-release",
    "planter-prod",
    "planter-go-live",
    "planter-ship",
    "planter-promote",
];

fn secrets() -> Example {
    let mut ex = Example::new(
        "deploy",
        "secrets",
        run_secrets,
        "Which secrets a deploy's driver is handed (M219, M220's review): each driver declares the names it \
         takes, the tokens the platform mints for its runs among them, and its environment holds and a box \
         hands it only those. A secret's name is an environment variable's (A-Z, 0-9 and _, 1 to 64, no digit \
         first) and never one that changes how the driver's shell, the dynamic linker or a runtime behaves: \
         a reserved name, prefix or suffix.",
        &[
            "arugula_e2e::deploy::driver_secrets",
            "arugula_e2e::deploy::secret_name_ok",
            "arugula_e2e::deploy::driver_takes",
            "arugula_e2e::deploy::RESERVED_SECRET_NAMES",
        ],
    )
    .shapes(
        "check: driver_secrets (driver), secret_name_ok (name) or driver_takes (driver, name)",
        "driver_secrets: secrets, the names in order, or null for no such driver; the others: ok",
    );
    for d in DRIVERS.iter().copied().chain(["nope", ""]) {
        ex.computed(&format!("the secrets of driver {d:?}"), json!({ "check": "driver_secrets", "driver": d }));
    }
    for (name, ok) in [
        ("APP_TOKEN", true),
        ("STRIPE_KEY", true),
        ("A", true),
        ("_X", true),
        (&*"A".repeat(64), true),
        (&*"A".repeat(65), false),
        ("", false),
        ("1PASSWORD", false),
        ("lower", false),
        ("WITH-DASH", false),
        ("WITH SPACE", false),
        ("PATH", false),
        ("HOME", false),
        ("IFS", false),
        ("BASH_ENV", false),
        ("PROMPT_COMMAND", false),
        ("NODE_OPTIONS", false),
        ("RUBYOPT", false),
        ("DEPLOY_ID", false),
        ("LD_PRELOAD", false),
        ("LC_ALL", false),
        ("ARUGULA_SOCK", false),
        ("SUDO_ASKPASS", false),
        ("GIT_SSH_COMMAND", false),
        ("NPM_CONFIG_REGISTRY", false),
        ("SSL_CERT_FILE", false),
        ("CURL_CA_BUNDLE", false),
        ("PYTHONPATH", false),
        ("HTTPS_PROXY", false),
        ("NO_PROXY", false),
        ("PROXY", true),
        ("MY_PATH", true),
    ] {
        ex.case(&format!("the name {name:?}"), json!({ "check": "secret_name_ok", "name": name }), json!({ "ok": ok }));
    }
    for (driver, name, ok) in [
        ("fake", "APP_TOKEN", true),
        ("fake", "FLY_API_TOKEN", false),
        ("chant-fly", "FLY_API_TOKEN", true),
        ("chant-fly", "GITHUB_TOKEN", true),
        ("chant-fly", "GIT_TOKEN", false),
        ("apps-platform", "APPS_PLATFORM_TOKEN", true),
        ("arugula-release", "RELEASE_TOKEN", true),
        ("planter-go-live", "PLANTER_APPS_TOKEN", true),
        ("planter-ship", "APP_REPO_TOKEN", false),
        ("nope", "APP_TOKEN", false),
    ] {
        ex.case(
            &format!("whether {driver} takes {name}"),
            json!({ "check": "driver_takes", "driver": driver, "name": name }),
            json!({ "ok": ok }),
        );
    }
    ex
}

// ---- planter ----

#[derive(Deserialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
enum PlanterIn {
    ProdNameOk { name: String },
    ShipNameOk { name: String },
    HostOk { host: String },
    PathOk { path: String },
    AppOf { r#where: String },
    ArtifactOk { reference: String },
    Refuses { driver: String, kind: String, reference: String, r#where: String },
    RendererDigest {},
    PromoteBranch { app: String },
}

fn run_planter(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(match input::<PlanterIn>(v)? {
        PlanterIn::ProdNameOk { name } => json!({ "ok": planter::prod_name_ok(&name) }),
        PlanterIn::ShipNameOk { name } => json!({ "ok": planter::ship_name_ok(&name) }),
        PlanterIn::HostOk { host } => json!({ "ok": planter::host_ok(&host) }),
        PlanterIn::PathOk { path } => json!({ "ok": planter::path_ok(&path) }),
        PlanterIn::AppOf { r#where } => json!({ "app": planter::app_of(&r#where) }),
        PlanterIn::ArtifactOk { reference } => json!({ "ok": planter::artifact_ok(&reference) }),
        PlanterIn::Refuses { driver, kind, reference, r#where } => {
            json!({ "refused": planter::refuses(&driver, &kind, &reference, &r#where) })
        }
        PlanterIn::RendererDigest {} => json!({ "digest": planter::renderer_digest() }),
        PlanterIn::PromoteBranch { app } => json!({ "branch": planter::promote_branch(&app) }),
    })
}

fn planter_names() -> Example {
    let mut ex = Example::new(
        "deploy",
        "planter",
        run_planter,
        "planter's flows as deploy drivers (M233): planter-prod (the pull request that readies an app for prod), \
         planter-go-live (its pointer, live at <name>.apps.arugula.io), planter-ship (an agent's tarball onto \
         loam) and planter-promote (the pull request that moves a shipped app into the cluster). Names: a prod \
         name is a host label of at most 42 (lowercase letters, digits, dashes, not first or last); a shipped \
         app's at most 32 and not reserved (the platform's, planter's, Flux's and the org's own names). Sources: \
         Ship an artifact by its digest (sha256:<64 lowercase hex>) from the platform's store, the others a \
         commit by its full id; Prod and Go live of arugula-salad/<app>, Promote of arugula-salad/shipped. The \
         renderer's digest, named in every plan, is SHA-256 over each vendored file's path, a zero byte, its \
         bytes and a zero byte, in the order platform-app/render.mjs, prod-scaffold.mjs, renderer.mjs (the files \
         are crates/e2e/src/planter/). Settings and plans are in drivers/.",
        &[
            "arugula_e2e::deploy::planter::prod_name_ok",
            "arugula_e2e::deploy::planter::ship_name_ok",
            "arugula_e2e::deploy::planter::host_ok",
            "arugula_e2e::deploy::planter::path_ok",
            "arugula_e2e::deploy::planter::app_of",
            "arugula_e2e::deploy::planter::artifact_ok",
            "arugula_e2e::deploy::planter::refuses",
            "arugula_e2e::deploy::planter::renderer_digest",
            "arugula_e2e::deploy::planter::promote_branch",
        ],
    )
    .shapes(
        "check: prod_name_ok (name), ship_name_ok (name), host_ok (host), path_ok (path), app_of (where), \
         artifact_ok (reference), refuses (driver, kind, reference, where), renderer_digest, or promote_branch (app)",
        "ok; app: the app's name or null; refused: why, or null when it can; digest; branch",
    );
    for (n, ok) in [
        ("ocarina", true),
        (&*"a".repeat(42), true),
        (&*"a".repeat(43), false),
        ("Caps", false),
        ("-x", false),
        ("x-", false),
        ("a_b", false),
        ("", false),
    ] {
        ex.case(&format!("prod name {n:?}"), json!({ "check": "prod_name_ok", "name": n }), json!({ "ok": ok }));
    }
    for (n, ok) in [
        ("hello", true),
        (&*"a".repeat(32), true),
        (&*"a".repeat(33), false),
        ("arugula", false),
        ("studio", false),
        ("platform", false),
        ("platform-x", false),
        ("platformx", true),
        ("planter", false),
        ("planterx", false),
        ("shipped", false),
        ("app", false),
        ("apps", false),
        ("flux-system", false),
        ("planter-apps", false),
        ("www", false),
        ("ghcr", false),
    ] {
        ex.case(&format!("shipped app name {n:?}"), json!({ "check": "ship_name_ok", "name": n }), json!({ "ok": ok }));
    }
    for (h, ok) in [
        ("shop.example.com", true),
        ("shop", false),
        ("Shop.example.com", false),
        ("-a.com", false),
        ("a-.com", false),
        ("a..com", false),
        (&*format!("{}.com", "a".repeat(64)), false),
    ] {
        ex.case(&format!("host {h:?}"), json!({ "check": "host_ok", "host": h }), json!({ "ok": ok }));
    }
    for (p, ok) in [("/health", true), ("/a/b.c_d~e-f", true), ("health", false), ("/a b", false), ("/a?b", false)] {
        ex.case(&format!("path {p:?}"), json!({ "check": "path_ok", "path": p }), json!({ "ok": ok }));
    }
    for (w, app) in [
        ("arugula-salad/ocarina", Some("ocarina")),
        ("acme/ocarina", None),
        ("arugula-salad/planter-apps", None),
        ("arugula-salad/shipped", None),
        ("arugula-salad/Caps", None),
        ("arugula-salad", None),
    ] {
        ex.case(&format!("the app of {w:?}"), json!({ "check": "app_of", "where": w }), json!({ "app": app }));
    }
    let digest = format!("sha256:{}", "a".repeat(64));
    for (r, ok) in
        [(&*digest, true), (&*digest.to_uppercase(), false), (&digest[..40], false), (&*"a".repeat(64), false)]
    {
        ex.case(&format!("artifact {r:?}"), json!({ "check": "artifact_ok", "reference": r }), json!({ "ok": ok }));
    }
    for (driver, kind, r, w) in [
        ("planter-go-live", "commit", SHA, "arugula-salad/ocarina"),
        ("planter-prod", "commit", SHA, "arugula-salad/ocarina"),
        ("planter-promote", "commit", SHA, "arugula-salad/shipped"),
        ("planter-ship", "artifact", &*digest, "platform"),
        ("planter-go-live", "commit", &SHA[..12], "arugula-salad/ocarina"),
        ("planter-go-live", "artifact", SHA, "arugula-salad/ocarina"),
        ("planter-go-live", "commit", SHA, "acme/ocarina"),
        ("planter-go-live", "commit", SHA, "arugula-salad/planter-apps"),
        ("planter-prod", "commit", SHA, "arugula-salad/Caps"),
        ("planter-promote", "commit", SHA, "arugula-salad/ocarina"),
        ("planter-ship", "commit", SHA, "platform"),
        ("planter-ship", "artifact", &*digest, "arugula-salad/shipped"),
    ] {
        ex.computed(
            &format!("{driver} deploying {kind} {r} of {w}"),
            json!({ "check": "refuses", "driver": driver, "kind": kind, "reference": r, "where": w }),
        );
    }
    ex.computed("the vendored renderer's digest", json!({ "check": "renderer_digest" }));
    ex.case(
        "a promotion's branch",
        json!({ "check": "promote_branch", "app": "hello" }),
        json!({ "branch": "promote/hello" }),
    );
    ex
}

// ---- apps-platform ----

#[derive(Deserialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
enum AppsIn {
    AppName { r#where: String },
    Refuses { kind: String, reference: String, r#where: String },
}

fn run_apps_platform(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(match input::<AppsIn>(v)? {
        AppsIn::AppName { r#where } => json!({ "app": apps_platform::app_name(&r#where) }),
        AppsIn::Refuses { kind, reference, r#where } => {
            json!({ "refused": apps_platform::refuses(&kind, &reference, &r#where) })
        }
    })
}

fn apps_platform_names() -> Example {
    let mut ex = Example::new(
        "deploy",
        "apps_platform",
        run_apps_platform,
        "The apps-platform driver (M222) deploys a commit of an app's repository by pinning its pointer in \
         arugula-salad/planter-apps: the app a source's where names is <list owner>/<name>, a name of 1 to 50 \
         lowercase letters, digits and dashes (not first or last) that the list doesn't keep for itself. It \
         pins a full commit id only.",
        &["arugula_e2e::deploy::apps_platform::app_name", "arugula_e2e::deploy::apps_platform::refuses"],
    )
    .shapes(
        "check: app_name (where) or refuses (kind, reference, where)",
        "app: the name or null; refused: why, or null",
    );
    for (w, app) in [
        ("arugula-salad/hello", Some("hello")),
        ("arugula-salad/planter-apps", None),
        ("arugula-salad/flux-system", None),
        ("arugula-salad/fleet", None),
        ("acme/hello", None),
        ("arugula-salad/Hello", None),
        ("arugula-salad/-x", None),
        ("arugula-salad/a/b", None),
    ] {
        ex.case(&format!("the app of {w:?}"), json!({ "check": "app_name", "where": w }), json!({ "app": app }));
    }
    for (kind, r, w) in [
        ("commit", SHA, "arugula-salad/hello"),
        ("artifact", SHA, "arugula-salad/hello"),
        ("commit", "main", "arugula-salad/hello"),
        ("commit", &*SHA.to_uppercase(), "arugula-salad/hello"),
        ("commit", SHA, "acme/hello"),
    ] {
        ex.computed(
            &format!("deploying {kind} {r} of {w}"),
            json!({ "check": "refuses", "kind": kind, "reference": r, "where": w }),
        );
    }
    ex
}
