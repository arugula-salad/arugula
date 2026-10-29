//! `deploy`: what a deploy's owner approves (the operation's bytes and
//! hash, the approval's signed bytes and passkey challenge), what a machine
//! checks before it applies one (`check_apply`), which secrets a driver
//! takes, and planter's and apps-platform's names and sources.

use std::collections::BTreeMap;

use arugula_e2e::{
    Kind,
    deploy::{
        Approval, Decision, Operation, PasskeyParts, apps_platform, check_apply, driver_secrets, driver_takes, planter,
        secret_name_ok,
    },
    team::TeamRole,
};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
    harness::{Ctx, Example, OWNERS_SHAPE, OwnersIn, Spec, cert, input, member, person, revoke},
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
        "A deploy's approval, or its deny, as an owner's device signs it: the deploy, the operation's hash, the \
         approver's account, the decision (once or deny), when the apply must start by (ms), and the signing \
         device, as a fixed text. Its sig is that device's Ed25519 signature over the body. well_formed: no field \
         is empty, longer than 200 bytes, or holds anything but printable ASCII without spaces, so the body reads \
         one way only. signed_by: well formed, and the signature is the given key's. Its passkey assertion's \
         WebAuthn challenge is SHA-256 of the body, base64url without padding.",
        &[
            "arugula_e2e::deploy::Approval::body",
            "arugula_e2e::deploy::Approval::well_formed",
            "arugula_e2e::deploy::Approval::signed_by",
            "arugula_e2e::deploy::Approval::passkey_challenge",
        ],
    )
    .shapes(
        "approval: an approval's label; sign: an Ed25519 public key, hex, to check its signature against",
        "body; well_formed; signed_by: whether sign signed it; passkey_challenge",
    );
    let alice = person(&mut ex, "alice", 1);
    let other = ex.key("someone-else", 2);
    let o = op(&plan('a'), "production");
    ex.object("op", &o);
    let once = Approval::new(&alice.keys, "dep_1", &o, "alice", Decision::Once, 90_000);
    let deny = Approval::new(&alice.keys, "dep_1", &o, "alice", Decision::Deny, 90_000);
    let pk = hex::encode(alice.keys.sign_public());
    let them = hex::encode(other.sign_public());
    let mut later = once.clone();
    later.expires = 99_000;
    let mut spaced = Approval::new(&alice.keys, "dep_1 dep_2", &o, "alice", Decision::Once, 90_000);
    spaced.account = "alice".into();
    let mut moved = once.clone();
    moved.deploy = "dep_2".into();
    for (l, a) in [
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
    case("alice approves the deploy once", "once", &pk, true, true);
    case("alice denies it: a deny is signed too", "deny", &pk, true, true);
    case("checked against another key", "once", &them, false, true);
    case("its expiry pushed out after she signed", "expires-changed", &pk, false, true);
    case("moved to another deploy after she signed", "other-deploy", &pk, false, true);
    case(
        "a space in a field: not well formed, so never signed_by, whoever signed it",
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
struct Passkey {
    client_data_json: String,
    authenticator_data: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyIn {
    approval: String,
    op: String,
    deploy: String,
    owners: OwnersIn,
    #[serde(default)]
    children: BTreeMap<String, Vec<String>>,
    passkey: Option<Passkey>,
    now: u64,
}

fn run_check_apply(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: ApplyIn = input(v)?;
    let owners = i.owners.load(ctx)?;
    let passkey = i
        .passkey
        .as_ref()
        .map(|p| PasskeyParts { client_data_json: &p.client_data_json, authenticator_data: &p.authenticator_data });
    let r =
        check_apply(&ctx.get(&i.approval)?, &ctx.get(&i.op)?, &i.deploy, &owners.owners(), &i.children, passkey, i.now);
    Ok(match r {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "refused": e.code() }),
    })
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A passkey assertion's client data and authenticator data over `challenge`.
fn passkey(kind: &str, challenge: &str, flags: u8, len: usize) -> Value {
    let cd = json!({ "type": kind, "challenge": challenge, "origin": "https://app.example.invalid" }).to_string();
    let mut data = Sha256::digest(b"app.example.invalid").to_vec();
    data.push(flags);
    data.extend([0, 0, 0, 7]);
    data.truncate(len);
    json!({ "client_data_json": b64(cd.as_bytes()), "authenticator_data": b64(&data) })
}

fn check_apply_cases() -> Example {
    let mut ex = Example::new(
        "deploy",
        "check_apply",
        run_check_apply,
        "What a machine checks before it applies a deploy (M219), from what it pinned rather than the platform's \
         word, in this order, the first failure being the refusal: the approval is well formed (approval_form); \
         for this deploy (approval_deploy); over exactly this operation's hash (approval_operation); once, not a \
         deny (approval_denied); not expired, expires > now (approval_expired); its account an owner of the \
         project, as the machine knows them, its team's roster or its own account (approval_not_an_owner); its \
         device trusted now from that owner's root, and a browser's or a phone's own device certified by \
         browsers (or recovery codes) back to a root that is a browser, never a CLI's or the app's \
         (approval_device; a grandfathered app window counts only just above a browser the hub says it \
         approved, `children`); the signature that device's (approval_signature); and a passkey assertion with \
         it: client data of type webauthn.get whose challenge is the approval's, authenticator data of at least \
         37 bytes with user presence and verification (flags & 0x05), base64url with or without padding \
         (approval_passkey). The passkey's own signature is the hub's to check.",
        &[
            "arugula_e2e::deploy::check_apply",
            "arugula_e2e::deploy::approver_chained",
            "arugula_e2e::deploy::passkey_binds",
        ],
    )
    .shapes(
        &format!(
            "approval and op: labels; deploy: the deploy the run applies; {OWNERS_SHAPE}; children: the browsers \
             grandfathered app windows approved, by account ({{ account: [device ids] }}); passkey: {{ \
             client_data_json, authenticator_data }}, each base64url, or null; now: the machine's clock, ms"
        ),
        "ok: true, or refused: the check's code",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let ak = &alice.keys;
    let k = |ex: &mut Example, l: &str, n| ex.key(l, n);
    let (cli, app, oldapp, code, recovered, phone, by_old, laundered, gone) = (
        k(&mut ex, "alice-cli", 10),
        k(&mut ex, "alice-app", 11),
        k(&mut ex, "alice-old-app", 12),
        k(&mut ex, "alice-recovery", 13),
        k(&mut ex, "alice-recovered-laptop", 14),
        k(&mut ex, "alice-phone", 15),
        k(&mut ex, "alice-phone-by-old-app", 16),
        k(&mut ex, "alice-browser-by-cli", 17),
        k(&mut ex, "alice-revoked-tablet", 18),
    );
    for (l, c) in [
        ("alice-cli", cert(&cli, "alice", Kind::Cli, "terminal", 1_100, ak)),
        ("alice-app", cert(&app, "alice", Kind::App, "Arugula app", 1_100, ak)),
        ("alice-old-app", cert(&oldapp, "alice", Kind::Browser, "Arugula app on jake-air", 1_100, ak)),
        ("alice-recovery", cert(&code, "alice", Kind::Recovery, "recovery 1", 1_100, ak)),
        ("alice-recovered-laptop", cert(&recovered, "alice", Kind::Browser, "new laptop", 1_200, &code)),
        ("alice-phone", cert(&phone, "alice", Kind::Browser, "phone", 1_100, ak)),
        ("alice-phone-by-old-app", cert(&by_old, "alice", Kind::Browser, "Safari on iPhone", 1_200, &oldapp)),
        ("alice-browser-by-cli", cert(&laundered, "alice", Kind::Browser, "laundered", 1_200, &cli)),
        ("alice-revoked-tablet", cert(&gone, "alice", Kind::Browser, "tablet", 1_100, ak)),
    ] {
        ex.object(l, &c);
    }
    ex.object("alice-tablet-revoked", &revoke("alice", &gone.id(), 5_000, ak));
    // A CLI-rooted account: its root is a CLI, so nothing of it approves.
    let cliroot = ex.key("carol-cli-root", 20);
    ex.object("carol-cli-root", &cert(&cliroot, "carol", Kind::Cli, "terminal", 1_000, &cliroot));
    let alice_certs: Vec<&str> = vec![
        "alice",
        "alice-cli",
        "alice-app",
        "alice-old-app",
        "alice-recovery",
        "alice-recovered-laptop",
        "alice-phone",
        "alice-phone-by-old-app",
        "alice-browser-by-cli",
        "alice-revoked-tablet",
    ];
    ex.object(
        "team-roster",
        &roster(1, 1_000, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Editor)], ak),
    );
    let account = json!({ "account": { "account": "alice", "root": alice.cert.device, "certs": alice_certs,
                          "revocations": ["alice-tablet-revoked"] } });
    let team = json!({ "team": { "roster": "team-roster", "certs": {
        "alice": { "certs": alice_certs, "revocations": ["alice-tablet-revoked"] },
        "bob": { "certs": ["bob"] } } } });
    let carol = json!({ "account": { "account": "carol", "root": cliroot.id(), "certs": ["carol-cli-root"] } });

    let o = op(&plan('a'), "production");
    ex.object("op", &o);
    ex.object("op-other-plan", &op(&plan('b'), "production"));
    ex.object("op-renamed", &op(&plan('a'), "prod"));
    ex.object("op-other-project", &Operation { project: "prj_2".into(), ..o.clone() });
    let now = 50_000;
    let approve = |ex: &mut Example, l: &str, by: &arugula_e2e::DeviceKeys, account: &str, d: Decision, exp: u64| {
        let a = Approval::new(by, "dep_1", &o, account, d, exp);
        ex.object(l, &a);
        a
    };
    let good = approve(&mut ex, "by-alice", ak, "alice", Decision::Once, 90_000);
    let pk = |a: &Approval| passkey("webauthn.get", &a.passkey_challenge(), 0x05, 37);
    let mut cases: Vec<(String, String, Value, Value, Value, Value)> = vec![];
    let mut add = |about: &str, a: &str, owners: &Value, passkey: Value, more: Value, want: Value| {
        cases.push((about.into(), a.into(), owners.clone(), passkey, more, want));
    };
    let ok = json!({ "ok": true });
    let no = |code: &str| json!({ "refused": code });
    let p = pk(&good);
    add(
        "alice, owner of her account's project, approves from her root browser",
        "by-alice",
        &account,
        p.clone(),
        json!({}),
        ok.clone(),
    );
    add("alice, owner of the team's project, approves", "by-alice", &team, p.clone(), json!({}), ok.clone());
    add(
        "checked for another deploy",
        "by-alice",
        &account,
        p.clone(),
        json!({ "deploy": "dep_2" }),
        no("approval_deploy"),
    );
    add(
        "checked against another plan",
        "by-alice",
        &account,
        p.clone(),
        json!({ "op": "op-other-plan" }),
        no("approval_operation"),
    );
    add(
        "checked against the environment renamed",
        "by-alice",
        &account,
        p.clone(),
        json!({ "op": "op-renamed" }),
        no("approval_operation"),
    );
    add(
        "checked against another project's operation",
        "by-alice",
        &account,
        p.clone(),
        json!({ "op": "op-other-project" }),
        no("approval_operation"),
    );
    let deny = approve(&mut ex, "deny-by-alice", ak, "alice", Decision::Deny, 90_000);
    add("a deny", "deny-by-alice", &account, pk(&deny), json!({}), no("approval_denied"));
    add("its expiry is now", "by-alice", &account, p.clone(), json!({ "now": 90_000 }), no("approval_expired"));
    add("1 ms before its expiry", "by-alice", &account, p.clone(), json!({ "now": 89_999 }), ok.clone());
    let by_bob = approve(&mut ex, "by-bob", &bob.keys, "bob", Decision::Once, 90_000);
    add("bob, an editor of the team, approves", "by-bob", &team, pk(&by_bob), json!({}), no("approval_not_an_owner"));
    add("bob approves alice's own project", "by-bob", &account, pk(&by_bob), json!({}), no("approval_not_an_owner"));
    let by_mallory = approve(&mut ex, "mallory-as-alice-by-bob", &bob.keys, "alice", Decision::Once, 90_000);
    add(
        "bob's device signs as alice",
        "mallory-as-alice-by-bob",
        &account,
        pk(&by_mallory),
        json!({}),
        no("approval_device"),
    );
    for (l, by, about, want) in [
        ("by-alice-cli", &cli, "from alice's CLI", no("approval_device")),
        ("by-alice-app", &app, "from alice's app", no("approval_device")),
        (
            "by-alice-old-app",
            &oldapp,
            "from the app's browser certificate from before it had a kind",
            no("approval_device"),
        ),
        ("by-alice-recovery", &code, "from alice's recovery code", no("approval_device")),
        ("by-alice-recovered", &recovered, "from a browser her recovery code let in", ok.clone()),
        ("by-alice-phone", &phone, "from a phone her root approved", ok.clone()),
        (
            "by-alice-phone-by-old-app",
            &by_old,
            "from a phone the old app window approved, with no children pinned",
            no("approval_device"),
        ),
        ("by-alice-browser-by-cli", &laundered, "from a browser her CLI made (never trusted)", no("approval_device")),
        ("by-alice-revoked-tablet", &gone, "from a tablet she revoked", no("approval_device")),
    ] {
        let a = approve(&mut ex, l, by, "alice", Decision::Once, 90_000);
        add(about, l, &account, pk(&a), json!({}), want);
    }
    let a = approve(&mut ex, "by-alice-phone-by-old-app", &by_old, "alice", Decision::Once, 90_000);
    add(
        "from a phone the old app window approved, the hub naming it among the window's children",
        "by-alice-phone-by-old-app",
        &account,
        pk(&a),
        json!({ "children": { "alice": [by_old.id()] } }),
        ok.clone(),
    );
    add(
        "the same, as alice's team's owner",
        "by-alice-phone-by-old-app",
        &team,
        pk(&a),
        json!({ "children": { "alice": [by_old.id()] } }),
        ok.clone(),
    );
    let a = approve(&mut ex, "by-alice-old-app-as-child", &oldapp, "alice", Decision::Once, 90_000);
    add(
        "from the old app window itself, the hub naming it among its own children",
        "by-alice-old-app-as-child",
        &account,
        pk(&a),
        json!({ "children": { "alice": [oldapp.id()] } }),
        no("approval_device"),
    );
    let a = approve(&mut ex, "by-carol-cli-root", &cliroot, "carol", Decision::Once, 90_000);
    add(
        "carol's account is rooted at a CLI: its root approves nothing",
        "by-carol-cli-root",
        &carol,
        pk(&a),
        json!({}),
        no("approval_device"),
    );
    let mut pushed = good.clone();
    pushed.expires = 99_000;
    ex.object("by-alice-expiry-pushed", &pushed);
    add(
        "its expiry pushed out after she signed",
        "by-alice-expiry-pushed",
        &account,
        pk(&pushed),
        json!({}),
        no("approval_signature"),
    );
    let mut forged = good.clone();
    forged.sig = hex::encode(bob.keys.signature(good.body().as_bytes()));
    ex.object("by-alice-signed-by-bob", &forged);
    add(
        "said to be by alice's device, signed by bob's",
        "by-alice-signed-by-bob",
        &account,
        pk(&forged),
        json!({}),
        no("approval_signature"),
    );
    let mut spaced = good.clone();
    spaced.deploy = "dep_1 dep_2".into();
    ex.object("space-in-deploy", &spaced);
    add(
        "a space in its deploy",
        "space-in-deploy",
        &account,
        p.clone(),
        json!({ "deploy": "dep_1 dep_2" }),
        no("approval_form"),
    );
    let mut empty = good.clone();
    empty.sig = String::new();
    ex.object("no-signature", &empty);
    add("no signature", "no-signature", &account, p.clone(), json!({}), no("approval_form"));
    let ch = good.passkey_challenge();
    let other = Approval::new(ak, "dep_9", &o, "alice", Decision::Once, 90_000).passkey_challenge();
    add("no passkey assertion", "by-alice", &account, Value::Null, json!({}), no("approval_passkey"));
    add(
        "a passkey made for a registration (webauthn.create)",
        "by-alice",
        &account,
        passkey("webauthn.create", &ch, 0x05, 37),
        json!({}),
        no("approval_passkey"),
    );
    add(
        "a passkey over another approval's challenge",
        "by-alice",
        &account,
        passkey("webauthn.get", &other, 0x05, 37),
        json!({}),
        no("approval_passkey"),
    );
    add(
        "a passkey with user presence only",
        "by-alice",
        &account,
        passkey("webauthn.get", &ch, 0x01, 37),
        json!({}),
        no("approval_passkey"),
    );
    add(
        "a passkey with user verification only",
        "by-alice",
        &account,
        passkey("webauthn.get", &ch, 0x04, 37),
        json!({}),
        no("approval_passkey"),
    );
    add(
        "a passkey with more flags set: fine",
        "by-alice",
        &account,
        passkey("webauthn.get", &ch, 0x45, 37),
        json!({}),
        ok.clone(),
    );
    add(
        "authenticator data of 36 bytes",
        "by-alice",
        &account,
        passkey("webauthn.get", &ch, 0x05, 36),
        json!({}),
        no("approval_passkey"),
    );
    let mut padded = passkey("webauthn.get", &ch, 0x05, 37);
    padded["client_data_json"] = json!(format!("{}==", padded["client_data_json"].as_str().unwrap()));
    add("its client data base64url with padding: fine", "by-alice", &account, padded, json!({}), ok.clone());
    let mut not_json = passkey("webauthn.get", &ch, 0x05, 37);
    not_json["client_data_json"] = json!(b64(b"not json"));
    add("client data that isn't JSON", "by-alice", &account, not_json, json!({}), no("approval_passkey"));
    let mut not_b64 = passkey("webauthn.get", &ch, 0x05, 37);
    not_b64["authenticator_data"] = json!("!!!");
    add("authenticator data that isn't base64url", "by-alice", &account, not_b64, json!({}), no("approval_passkey"));
    for (about, a, owners, passkey, more, want) in cases {
        let mut input = json!({ "approval": a, "op": "op", "deploy": "dep_1", "owners": owners, "children": {},
                                "passkey": passkey, "now": now });
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
