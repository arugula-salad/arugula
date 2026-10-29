//! `hosted`: hosted machines (M178), enrolled into one project by control
//! under that project's delegation: the papers' bytes, what the hub signs
//! (a machine's certificate, with control's hosted key), which client
//! accepts such a machine (`accept`, every refusal), whether control may
//! enroll under a delegation now (`live`), and project owners' names.

use arugula_e2e::{
    DeviceKeys, Kind,
    hosted::{Delegation, DelegationRevoke, HostedCert, HostedKey, Owner, accept, enroll_proof_body, live},
    team::TeamRole,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    harness::{Ctx, Example, OWNERS_SHAPE, OwnersIn, Spec, cert, input, keys, member, person, revoke},
    team::roster,
};

pub const FILES: &[Spec] = &[
    Spec { module: "hosted", name: "bodies", run: run_bodies, make: bodies },
    Spec { module: "hosted", name: "sign", run: run_sign, make: signing },
    Spec { module: "hosted", name: "accept", run: run_accept, make: accepts },
    Spec { module: "hosted", name: "live", run: run_live, make: lives },
    Spec { module: "hosted", name: "owner", run: run_owner, make: owners },
];

/// Control's hosted key from byte `n`, noted in the file.
fn hosted_key(ex: &mut Example, label: &str, n: u8) -> HostedKey {
    let k = HostedKey::from_seed(&[n; 32]);
    ex.key_value(label, json!({ "seed": hex::encode([n; 32]), "sign": k.public() }));
    k
}

fn delegation(project: &str, owner: &str, key: &HostedKey, at: u64, by: &DeviceKeys) -> Delegation {
    let mut d = Delegation {
        project: project.into(),
        owner: owner.into(),
        key: key.public(),
        at,
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(by);
    d
}

fn machine(d: &Delegation, workspace: &str, k: &DeviceKeys, created: u64, key: &HostedKey) -> HostedCert {
    let mut c =
        HostedCert::new(d, workspace, "box", &hex::encode(k.noise_public), &hex::encode(k.sign_public()), created);
    key.sign(&mut c);
    c
}

// ---- bodies ----

#[derive(Deserialize)]
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
enum BodyIn {
    Delegation { delegation: String },
    Revoke { revoke: String },
    Machine { cert: String },
    EnrollProof { credential: String, noise: String, sign: String, ms: u64 },
}

fn run_bodies(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(match input::<BodyIn>(v)? {
        BodyIn::Delegation { delegation } => {
            let d: Delegation = ctx.get(&delegation)?;
            json!({ "body": d.body(), "id": d.id() })
        }
        BodyIn::Revoke { revoke } => json!({ "body": ctx.get::<DelegationRevoke>(&revoke)?.body() }),
        BodyIn::Machine { cert } => json!({ "body": ctx.get::<HostedCert>(&cert)?.body() }),
        BodyIn::EnrollProof { credential, noise, sign, ms } => {
            json!({ "body": enroll_proof_body(&credential, &noise, &sign, ms) })
        }
    })
}

fn bodies() -> Example {
    let mut ex = Example::new(
        "hosted",
        "bodies",
        run_bodies,
        "The exact bytes of a hosted machine's papers. A delegation: an owner's device says control may enroll \
         hosted machines into a project with its hosted key, until revoked; its sig is that device's over the \
         body, and its id the first 16 bytes of SHA-256 over the body, hex. A revoke names the delegation by \
         that id. A machine's certificate is signed with the key the delegation names. A machine signs an enroll \
         proof with its own key: the SHA-256 (hex) of the ticket or sandbox id it enrolls with, so the proof \
         can't move to another enrollment and the credential isn't in it.",
        &[
            "arugula_e2e::hosted::Delegation::body",
            "arugula_e2e::hosted::Delegation::id",
            "arugula_e2e::hosted::DelegationRevoke::body",
            "arugula_e2e::hosted::HostedCert::body",
            "arugula_e2e::hosted::enroll_proof_body",
        ],
    )
    .shapes(
        "of: delegation (delegation: a label), revoke (revoke), machine (cert), or enroll_proof (credential, noise, \
         sign, ms)",
        "body: the signed text; id: a delegation's",
    );
    let alice = person(&mut ex, "alice", 1);
    let key = hosted_key(&mut ex, "control-hosted-key", 50);
    let boxk = ex.key("box", 2);
    let d = delegation("prj_1", "account:alice", &key, 2_000, &alice.keys);
    ex.object("delegation", &d);
    ex.object("team-delegation", &delegation("prj_2", "team:t1", &key, 2_000, &alice.keys));
    ex.object("revoke", &DelegationRevoke::new(&d, 4_000, &alice.keys));
    ex.object("machine", &machine(&d, "hw_1", &boxk, 3_000, &key));
    ex.computed("an account's project's delegation", json!({ "of": "delegation", "delegation": "delegation" }));
    ex.computed("a team's project's", json!({ "of": "delegation", "delegation": "team-delegation" }));
    ex.computed("its revoke", json!({ "of": "revoke", "revoke": "revoke" }));
    ex.computed("a machine's certificate", json!({ "of": "machine", "cert": "machine" }));
    let credential = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b"ticket-of-the-example"));
    ex.computed(
        "a machine's enroll proof",
        json!({ "of": "enroll_proof", "credential": credential, "noise": hex::encode(boxk.noise_public),
                "sign": hex::encode(boxk.sign_public()), "ms": 3_000 }),
    );
    ex
}

// ---- the hub's signing ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignIn {
    seed: String,
    delegation: String,
    workspace: String,
    name: String,
    noise: String,
    sign: String,
    created: u64,
}

fn run_sign(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: SignIn = input(v)?;
    let seed: [u8; 32] = hex::decode(&i.seed)?.try_into().map_err(|_| anyhow::anyhow!("a seed is 32 bytes"))?;
    let key = HostedKey::from_seed(&seed);
    let d: Delegation = ctx.get(&i.delegation)?;
    let mut c = HostedCert::new(&d, &i.workspace, &i.name, &i.noise, &i.sign, i.created);
    key.sign(&mut c);
    Ok(json!({ "public": key.public(), "cert": c, "form_ok": c.check_form().is_ok() }))
}

fn signing() -> Example {
    let mut ex = Example::new(
        "hosted",
        "sign",
        run_sign,
        "What the hub signs when it enrolls a hosted machine: its certificate under the project's delegation \
         (the project, the owner and the delegation's id from it; the device id from the machine's keys, or \
         empty when they aren't 32 bytes of hex each), with control's hosted key, an Ed25519 key from a 32-byte \
         seed. Ed25519 is deterministic, so a hub written anew must make these bytes exactly. form_ok is \
         HostedCert::check_form: the ids tokens, the owner team:<id> or account:<id>, a name of 1 to 64 \
         characters with no control character, and the device id its keys'.",
        &[
            "arugula_e2e::hosted::HostedKey::from_seed",
            "arugula_e2e::hosted::HostedKey::public",
            "arugula_e2e::hosted::HostedKey::sign",
            "arugula_e2e::hosted::HostedCert::new",
            "arugula_e2e::hosted::HostedCert::check_form",
        ],
    )
    .shapes(
        "seed: the hosted key's seed, hex; delegation: a label; workspace, name, noise, sign: the machine's; created: ms",
        "public: the hosted key's public half, hex; cert: the signed certificate; form_ok",
    );
    let alice = person(&mut ex, "alice", 1);
    let key = hosted_key(&mut ex, "control-hosted-key", 50);
    let boxk = ex.key("box", 2);
    let d = delegation("prj_1", "account:alice", &key, 2_000, &alice.keys);
    ex.object("delegation", &d);
    ex.object("team-delegation", &delegation("prj_2", "team:t1", &key, 2_000, &alice.keys));
    let (noise, sign) = (hex::encode(boxk.noise_public), hex::encode(boxk.sign_public()));
    let seed = hex::encode([50u8; 32]);
    let mut case = |about: &str, d: &str, ws: &str, name: &str, noise: &str, sign: &str, ok: bool| {
        let got = ex.computed(
            about,
            json!({ "seed": seed, "delegation": d, "workspace": ws, "name": name, "noise": noise, "sign": sign,
                    "created": 3_000 }),
        );
        assert_eq!(got["form_ok"], json!(ok), "{about}");
    };
    case("a box of an account's project", "delegation", "hw_1", "box", &noise, &sign, true);
    case("a box of a team's project", "team-delegation", "hw_2", "box", &noise, &sign, true);
    case("another workspace's", "delegation", "hw_3", "build box", &noise, &sign, true);
    case("keys that aren't hex: no device id", "delegation", "hw_1", "box", "zz", &sign, false);
    case("a newline in the name", "delegation", "hw_1", "box\nx", &noise, &sign, false);
    case("a space in the workspace", "delegation", "hw 1", "box", &noise, &sign, false);
    ex
}

// ---- accept ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptIn {
    me: String,
    cert: String,
    delegation: String,
    #[serde(default)]
    revokes: Vec<String>,
    owners: OwnersIn,
}

fn run_accept(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: AcceptIn = input(v)?;
    let owners = i.owners.load(ctx)?;
    let revokes: Vec<DelegationRevoke> = ctx.all(&i.revokes)?;
    Ok(match accept(&i.me, &ctx.get(&i.cert)?, &ctx.get(&i.delegation)?, &revokes, &owners.owners()) {
        Ok(a) => json!({ "accepted": {
            "project": a.project,
            "owner": a.owner.as_string(),
            "workspace": a.workspace,
            "role": a.role.as_str(),
        } }),
        Err(e) => json!({ "refused": e.code() }),
    })
}

fn accepts() -> Example {
    let mut ex = Example::new(
        "hosted",
        "accept",
        run_accept,
        "Whether a client accepts a hosted machine, for that project only: the client's account (me) and what it \
         knows of the project's owners from its own checks (a team's roster and members' certificates, or its \
         account's chain), never control's word for the machine. In this order: the certificate and delegation \
         well formed and naming each other (form); the owners the client knows are the delegation's (wrong_project); \
         me in the project (not_in_project); the delegation signed by a device, trusted now, of one of the \
         project's owners, a browser, a CLI or the app (not a machine or a recovery code) (unsigned); the \
         certificate signed with the key the delegation names (bad_signature); made no earlier than the \
         delegation and before any revoke that checks out (by any owner, of this delegation) (revoked). \
         Revokes that don't check out are ignored. Accepted, it gives the client's role in the project.",
        &["arugula_e2e::hosted::accept", "arugula_e2e::hosted::HostedRefusal::code"],
    )
    .shapes(
        &format!("me: the client's account; cert, delegation: labels; revokes: labels; {OWNERS_SHAPE}"),
        "accepted: { project, owner, workspace, role }, or refused: the check's code",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let carol = person(&mut ex, "carol", 3);
    let vic = person(&mut ex, "vic", 4);
    let mallory = person(&mut ex, "mallory", 5);
    let ak = &alice.keys;
    let key = hosted_key(&mut ex, "control-hosted-key", 50);
    let other_key = hosted_key(&mut ex, "another-hosted-key", 51);
    let boxk = ex.key("box", 6);
    let k = |ex: &mut Example, l: &str, n| ex.key(l, n);
    let (cli, app, code, mach, gone) = (
        k(&mut ex, "alice-cli", 10),
        k(&mut ex, "alice-app", 11),
        k(&mut ex, "alice-recovery", 12),
        k(&mut ex, "alice-machine", 13),
        k(&mut ex, "alice-revoked-tablet", 14),
    );
    for (l, c) in [
        ("alice-cli", cert(&cli, "alice", Kind::Cli, "terminal", 1_100, ak)),
        ("alice-app", cert(&app, "alice", Kind::App, "Arugula app", 1_100, ak)),
        ("alice-recovery", cert(&code, "alice", Kind::Recovery, "recovery 1", 1_100, ak)),
        ("alice-machine", cert(&mach, "alice", Kind::Daemon, "geek", 1_100, ak)),
        ("alice-revoked-tablet", cert(&gone, "alice", Kind::Browser, "tablet", 1_100, ak)),
    ] {
        ex.object(l, &c);
    }
    ex.object("alice-tablet-revoked", &revoke("alice", &gone.id(), 1_500, ak));
    let alice_certs = ["alice", "alice-cli", "alice-app", "alice-recovery", "alice-machine", "alice-revoked-tablet"];
    let mine = json!({ "account": { "account": "alice", "root": alice.cert.device, "certs": alice_certs,
                       "revocations": ["alice-tablet-revoked"] } });
    ex.object(
        "roster",
        &roster(
            1,
            1_000,
            vec![
                member(&alice, TeamRole::Owner),
                member(&carol, TeamRole::Owner),
                member(&bob, TeamRole::Editor),
                member(&vic, TeamRole::Viewer),
            ],
            ak,
        ),
    );
    let team = json!({ "team": { "roster": "roster", "certs": {
        "alice": { "certs": alice_certs, "revocations": ["alice-tablet-revoked"] },
        "bob": { "certs": ["bob"] }, "carol": { "certs": ["carol"] }, "vic": { "certs": ["vic"] } } } });
    let bobs = json!({ "account": { "account": "bob", "root": bob.cert.device, "certs": ["bob"] } });

    // The delegations and the machines under them.
    let acct = "account:alice";
    let d = delegation("prj_1", acct, &key, 2_000, ak);
    ex.object("d", &d);
    ex.object("box", &machine(&d, "hw_1", &boxk, 3_000, &key));
    let td = delegation("prj_2", "team:t1", &key, 2_000, ak);
    ex.object("team-d", &td);
    ex.object("team-box", &machine(&td, "hw_2", &boxk, 3_000, &key));
    // Each case's about, input and output, made as it's added.
    let mut cases: Vec<(String, Value, Value)> = vec![];
    let mut add = |about: &str, me: &str, c: &str, dl: &str, revs: &[&str], owners: &Value, want: Value| {
        let input = json!({ "me": me, "cert": c, "delegation": dl, "revokes": revs, "owners": owners });
        cases.push((about.into(), input, want));
    };
    let yes = |owner: &str, project: &str, ws: &str, role: &str| json!({ "accepted": { "project": project, "owner": owner, "workspace": ws, "role": role } });
    let no = |code: &str| json!({ "refused": code });
    add("alice's own project's box", "alice", "box", "d", &[], &mine, yes(acct, "prj_1", "hw_1", "owner"));
    for (me, role) in [("alice", "owner"), ("carol", "owner"), ("bob", "editor"), ("vic", "viewer")] {
        add(
            &format!("the team's box, to {me}"),
            me,
            "team-box",
            "team-d",
            &[],
            &team,
            yes("team:t1", "prj_2", "hw_2", role),
        );
    }
    add(
        "the team's box, to mallory, who isn't in the team",
        "mallory",
        "team-box",
        "team-d",
        &[],
        &team,
        no("not_in_project"),
    );
    add("alice's box, to bob", "bob", "box", "d", &[], &mine, no("not_in_project"));

    // Form.
    let put = |ex: &mut Example, l: &str, c: &HostedCert| ex.object(l, c);
    let mut moved = machine(&d, "hw_1", &boxk, 3_000, &key);
    moved.project = "prj_9".into();
    key.sign(&mut moved);
    put(&mut ex, "box-of-another-project", &moved);
    add(
        "a box whose certificate names another project",
        "alice",
        "box-of-another-project",
        "d",
        &[],
        &mine,
        no("form"),
    );
    let mut other_owner = machine(&d, "hw_1", &boxk, 3_000, &key);
    other_owner.owner = "account:bob".into();
    key.sign(&mut other_owner);
    put(&mut ex, "box-of-another-owner", &other_owner);
    add("a box whose certificate names another owner", "alice", "box-of-another-owner", "d", &[], &mine, no("form"));
    put(&mut ex, "team-box", &machine(&td, "hw_2", &boxk, 3_000, &key));
    add("a box made under another delegation", "alice", "team-box", "d", &[], &mine, no("form"));
    let mut wrong_id = machine(&d, "hw_1", &boxk, 3_000, &key);
    wrong_id.device = keys(7).id();
    key.sign(&mut wrong_id);
    put(&mut ex, "box-with-another-id", &wrong_id);
    add("a box whose device id isn't its keys'", "alice", "box-with-another-id", "d", &[], &mine, no("form"));
    let mut nl = machine(&d, "hw_1", &boxk, 3_000, &key);
    nl.name = "box\nproject prj_9".into();
    key.sign(&mut nl);
    put(&mut ex, "box-newline-in-name", &nl);
    add("a newline in the box's name", "alice", "box-newline-in-name", "d", &[], &mine, no("form"));
    let mut org = delegation("prj_1", "org:acme", &key, 2_000, ak);
    ex.object("d-org-owner", &org);
    let mut org_box = machine(&org, "hw_1", &boxk, 3_000, &key);
    org_box.owner = "org:acme".into();
    key.sign(&mut org_box);
    put(&mut ex, "box-org-owner", &org_box);
    add(
        "a delegation whose owner is neither a team nor an account",
        "alice",
        "box-org-owner",
        "d-org-owner",
        &[],
        &mine,
        no("form"),
    );
    org.key = "zz".into();
    org.owner = acct.into();
    org.sign_with(ak);
    ex.object("d-key-not-hex", &org);
    let mut nk =
        HostedCert::new(&org, "hw_1", "box", &hex::encode(boxk.noise_public), &hex::encode(boxk.sign_public()), 3_000);
    key.sign(&mut nk);
    put(&mut ex, "box-under-d-key-not-hex", &nk);
    add(
        "a delegation whose key isn't hex",
        "alice",
        "box-under-d-key-not-hex",
        "d-key-not-hex",
        &[],
        &mine,
        no("form"),
    );

    // Wrong project.
    add(
        "the team's box, checked against alice's own account",
        "alice",
        "team-box",
        "team-d",
        &[],
        &mine,
        no("wrong_project"),
    );
    add("alice's box, checked against the team", "alice", "box", "d", &[], &team, no("wrong_project"));
    add("alice's box, checked against bob's account", "bob", "box", "d", &[], &bobs, no("wrong_project"));

    // Unsigned.
    for (l, by, about, want) in [
        ("d-by-cli", &cli, "a delegation alice's CLI signed", None),
        ("d-by-app", &app, "a delegation alice's app signed", None),
        ("d-by-recovery", &code, "a delegation alice's recovery code signed", Some("unsigned")),
        ("d-by-machine", &mach, "a delegation a machine of alice's signed", Some("unsigned")),
        ("d-by-revoked-tablet", &gone, "a delegation a tablet she revoked signed", Some("unsigned")),
        ("d-by-bob", &bob.keys, "a delegation for alice's project that bob signed", Some("unsigned")),
        ("d-by-mallory", &mallory.keys, "a delegation for alice's project that mallory signed", Some("unsigned")),
    ] {
        let dd = delegation("prj_1", acct, &key, 2_000, by);
        ex.object(l, &dd);
        let bl = format!("box-under-{l}");
        put(&mut ex, &bl, &machine(&dd, "hw_1", &boxk, 3_000, &key));
        add(about, "alice", &bl, l, &[], &mine, want.map_or_else(|| yes(acct, "prj_1", "hw_1", "owner"), no));
    }
    for (l, by, about, want) in [
        ("team-d-by-carol", &carol.keys, "a team delegation the team's other owner signed", None),
        ("team-d-by-bob", &bob.keys, "a team delegation an editor signed", Some("unsigned")),
        ("team-d-by-vic", &vic.keys, "a team delegation a viewer signed", Some("unsigned")),
    ] {
        let dd = delegation("prj_2", "team:t1", &key, 2_000, by);
        ex.object(l, &dd);
        let bl = format!("box-under-{l}");
        put(&mut ex, &bl, &machine(&dd, "hw_2", &boxk, 3_000, &key));
        add(about, "vic", &bl, l, &[], &team, want.map_or_else(|| yes("team:t1", "prj_2", "hw_2", "viewer"), no));
    }
    let mut swapped = d.clone();
    swapped.key = other_key.public();
    ex.object("d-key-swapped", &swapped);
    put(&mut ex, "box-under-d-key-swapped", &machine(&swapped, "hw_1", &boxk, 3_000, &other_key));
    add(
        "control swaps its own key into alice's delegation",
        "alice",
        "box-under-d-key-swapped",
        "d-key-swapped",
        &[],
        &mine,
        no("unsigned"),
    );
    let mut later = d.clone();
    later.at = 1_000;
    ex.object("d-backdated", &later);
    put(&mut ex, "box-under-d-backdated", &machine(&later, "hw_1", &boxk, 3_000, &key));
    add(
        "control backdates alice's delegation",
        "alice",
        "box-under-d-backdated",
        "d-backdated",
        &[],
        &mine,
        no("unsigned"),
    );

    // Bad signature.
    let mut by_other =
        HostedCert::new(&d, "hw_1", "box", &hex::encode(boxk.noise_public), &hex::encode(boxk.sign_public()), 3_000);
    other_key.sign(&mut by_other);
    put(&mut ex, "box-signed-with-another-key", &by_other);
    add(
        "a box signed with a key the delegation doesn't name",
        "alice",
        "box-signed-with-another-key",
        "d",
        &[],
        &mine,
        no("bad_signature"),
    );
    let mut ws = machine(&d, "hw_1", &boxk, 3_000, &key);
    ws.workspace = "hw_9".into();
    put(&mut ex, "box-workspace-changed", &ws);
    add(
        "a box moved to another workspace after it was signed",
        "alice",
        "box-workspace-changed",
        "d",
        &[],
        &mine,
        no("bad_signature"),
    );
    let mut swapped_keys = machine(&d, "hw_1", &boxk, 3_000, &key);
    let mk = keys(8);
    swapped_keys.noise = hex::encode(mk.noise_public);
    swapped_keys.sign = hex::encode(mk.sign_public());
    swapped_keys.device = mk.id();
    put(&mut ex, "box-keys-swapped", &swapped_keys);
    add(
        "a box's keys swapped for another machine's after it was signed",
        "alice",
        "box-keys-swapped",
        "d",
        &[],
        &mine,
        no("bad_signature"),
    );

    // Revoked, by time.
    put(&mut ex, "box-before-d", &machine(&d, "hw_1", &boxk, 1_999, &key));
    add("a box made before its delegation", "alice", "box-before-d", "d", &[], &mine, no("revoked"));
    put(&mut ex, "box-at-d", &machine(&d, "hw_1", &boxk, 2_000, &key));
    add(
        "a box made the very ms of its delegation",
        "alice",
        "box-at-d",
        "d",
        &[],
        &mine,
        yes(acct, "prj_1", "hw_1", "owner"),
    );
    for (l, at, by) in [
        ("revoke-at-4000", 4_000, ak),
        ("revoke-at-3000", 3_000, ak),
        ("revoke-at-2500", 2_500, ak),
        ("revoke-by-cli", 2_500, &cli),
        ("revoke-by-recovery", 2_500, &code),
        ("revoke-by-bob", 2_500, &bob.keys),
    ] {
        ex.object(l, &DelegationRevoke::new(&d, at, by));
    }
    let mut bad = DelegationRevoke::new(&d, 2_500, ak);
    bad.at = 2_400;
    ex.object("revoke-bad-signature", &bad);
    ex.object("revoke-of-another-delegation", &DelegationRevoke::new(&td, 2_500, ak));
    let ok = yes(acct, "prj_1", "hw_1", "owner");
    add("revoked after the box was made: the box stays", "alice", "box", "d", &["revoke-at-4000"], &mine, ok.clone());
    add("revoked the very ms the box was made", "alice", "box", "d", &["revoke-at-3000"], &mine, no("revoked"));
    add("revoked before the box was made", "alice", "box", "d", &["revoke-at-2500"], &mine, no("revoked"));
    add(
        "two revokes: the earlier counts",
        "alice",
        "box",
        "d",
        &["revoke-at-4000", "revoke-at-2500"],
        &mine,
        no("revoked"),
    );
    add("revoked by alice's CLI: it signs for projects", "alice", "box", "d", &["revoke-by-cli"], &mine, no("revoked"));
    add("a revoke her recovery code signed: ignored", "alice", "box", "d", &["revoke-by-recovery"], &mine, ok.clone());
    add("a revoke bob signed: ignored", "alice", "box", "d", &["revoke-by-bob"], &mine, ok.clone());
    add(
        "a revoke whose signature doesn't check: ignored",
        "alice",
        "box",
        "d",
        &["revoke-bad-signature"],
        &mine,
        ok.clone(),
    );
    add(
        "a revoke of another delegation: ignored",
        "alice",
        "box",
        "d",
        &["revoke-of-another-delegation"],
        &mine,
        ok.clone(),
    );
    for (l, by) in [("team-revoke-by-carol", &carol.keys), ("team-revoke-by-bob", &bob.keys)] {
        ex.object(l, &DelegationRevoke::new(&td, 2_500, by));
    }
    add(
        "the team's delegation revoked by its other owner (not the one who delegated)",
        "vic",
        "team-box",
        "team-d",
        &["team-revoke-by-carol"],
        &team,
        no("revoked"),
    );
    add(
        "the team's delegation revoked by an editor: ignored",
        "vic",
        "team-box",
        "team-d",
        &["team-revoke-by-bob"],
        &team,
        yes("team:t1", "prj_2", "hw_2", "viewer"),
    );
    for (about, input, want) in cases {
        ex.case(&about, input, want);
    }
    ex
}

// ---- live ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveIn {
    delegation: String,
    #[serde(default)]
    revokes: Vec<String>,
    owners: OwnersIn,
}

fn run_live(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: LiveIn = input(v)?;
    let owners = i.owners.load(ctx)?;
    let revokes: Vec<DelegationRevoke> = ctx.all(&i.revokes)?;
    Ok(json!({ "live": live(&ctx.get(&i.delegation)?, &revokes, &owners.owners()) }))
}

fn lives() -> Example {
    let mut ex = Example::new(
        "hosted",
        "live",
        run_live,
        "What control checks before it signs a machine under a delegation: it's signed for the project's owners \
         (as accept's unsigned check) and nothing that checks out revokes it, whenever the revoke was made. \
         Clients check again, with what they hold (hosted/accept).",
        &[
            "arugula_e2e::hosted::live",
            "arugula_e2e::hosted::Delegation::signed_for",
            "arugula_e2e::hosted::DelegationRevoke::revokes",
        ],
    )
    .shapes(&format!("delegation: a label; revokes: labels; {OWNERS_SHAPE}"), "live");
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let key = hosted_key(&mut ex, "control-hosted-key", 50);
    let d = delegation("prj_1", "account:alice", &key, 2_000, &alice.keys);
    ex.object("d", &d);
    ex.object("d-by-bob", &delegation("prj_1", "account:alice", &key, 2_000, &bob.keys));
    ex.object("revoke", &DelegationRevoke::new(&d, 9_000_000, &alice.keys));
    ex.object("revoke-by-bob", &DelegationRevoke::new(&d, 2_500, &bob.keys));
    let mut other_project = DelegationRevoke::new(&d, 2_500, &alice.keys);
    other_project.project = "prj_9".into();
    other_project.sig = hex::encode(alice.keys.signature(other_project.body().as_bytes()));
    ex.object("revoke-other-project", &other_project);
    let mine = json!({ "account": { "account": "alice", "root": alice.cert.device, "certs": ["alice"] } });
    let case = |ex: &mut Example, about: &str, dl: &str, revs: &[&str], ok: bool| {
        ex.case(about, json!({ "delegation": dl, "revokes": revs, "owners": mine }), json!({ "live": ok }));
    };
    case(&mut ex, "alice's delegation", "d", &[], true);
    case(&mut ex, "revoked, however far in the future the revoke says", "d", &["revoke"], false);
    case(&mut ex, "a revoke bob signed: ignored", "d", &["revoke-by-bob"], true);
    case(&mut ex, "a revoke naming another project: ignored", "d", &["revoke-other-project"], true);
    case(&mut ex, "a delegation bob signed for alice's project", "d-by-bob", &[], false);
    ex
}

// ---- owner ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerIn {
    owner: String,
}

fn run_owner(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: OwnerIn = input(v)?;
    Ok(json!({ "owner": Owner::parse(&i.owner).map(|o| o.as_string()) }))
}

fn owners() -> Example {
    let mut ex = Example::new(
        "hosted",
        "owner",
        run_owner,
        "Who a project belongs to, as delegations and hosted certificates sign it: team:<id> or account:<id>, the \
         id 1 to 64 of a-z, A-Z, 0-9, - and _.",
        &["arugula_e2e::hosted::Owner::parse", "arugula_e2e::hosted::Owner::as_string"],
    )
    .shapes("owner: the text", "owner: it again, or null when it isn't one");
    for (s, ok) in [
        ("team:t1", true),
        ("account:acct_1-x", true),
        ("org:acme", false),
        ("team:", false),
        ("team:a b", false),
        ("team:t1:x", false),
        ("team", false),
        ("Team:t1", false),
        (&*format!("account:{}", "a".repeat(64)), true),
        (&*format!("account:{}", "a".repeat(65)), false),
    ] {
        ex.case(&format!("{s:?}"), json!({ "owner": s }), json!({ "owner": ok.then_some(s) }));
    }
    ex
}
