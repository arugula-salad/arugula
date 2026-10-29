//! `team`: who is in a team and as what, signed by its owners, so control
//! can't add a member: who may sign a paper that makes someone an owner or
//! an editor (`raise_ok`, `Roster::raised`), which roster may follow which
//! (`Roster::follows`, presigned invites included), and the papers beside
//! them (*Ask me first* invites, a daemon's team join, a machine's move).

use std::collections::BTreeMap;

use arugula_e2e::{
    Cert, DeviceKeys, Kind, Revocation,
    team::{AccountCerts, AskInvite, Invite, Member, Move, Redeem, Roster, Spent, TeamPin, TeamRole, raise_ok},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::harness::{Ctx, Example, Spec, cert, input, revoke};

pub const FILES: &[Spec] = &[
    Spec { module: "team", name: "raise_ok", run: run_raise_ok, make: raise_ok_table },
    Spec { module: "team", name: "raised", run: run_raised, make: raised },
    Spec { module: "team", name: "follows", run: run_follows, make: follows },
    Spec { module: "team", name: "bodies", run: run_bodies, make: bodies },
    Spec { module: "team", name: "ask_invite", run: run_ask, make: ask_invite },
    Spec { module: "team", name: "join", run: run_join, make: join },
    Spec { module: "team", name: "move", run: run_move, make: moves },
    Spec { module: "team", name: "devices", run: run_devices, make: devices },
];

const TEAM: &str = "t1";

fn role(r: Option<TeamRole>) -> Value {
    json!(r.map(TeamRole::as_str))
}

fn parse_role(s: &str) -> anyhow::Result<TeamRole> {
    Ok(serde_json::from_value(json!(s))?)
}

/// An account's certificates and revocations, by label, as an input lists them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Account {
    certs: Vec<String>,
    #[serde(default)]
    revocations: Vec<String>,
}

fn account_certs(ctx: &Ctx, a: &BTreeMap<String, Account>) -> anyhow::Result<AccountCerts> {
    a.iter()
        .map(|(k, v)| Ok((k.clone(), (ctx.all::<Cert>(&v.certs)?, ctx.all::<Revocation>(&v.revocations)?))))
        .collect()
}

const CERTS_SHAPE: &str = "certs: every member account's certificates and revocations, as control hands them \
                           out: { account: { certs: [labels], revocations: [labels] } }";

// ---- people ----

/// Someone with a browser as their account's root.
struct Person {
    account: &'static str,
    keys: DeviceKeys,
    cert: Cert,
}

fn person(ex: &mut Example, account: &'static str, n: u8) -> Person {
    let keys = ex.key(account, n);
    let cert = cert(&keys, account, Kind::Browser, &format!("{account}'s laptop"), 1_000, &keys);
    ex.object(account, &cert);
    Person { account, keys, cert }
}

fn member(p: &Person, role: TeamRole) -> Member {
    Member { account: p.account.into(), root: p.cert.device.clone(), role, name: format!("{}-login", p.account) }
}

fn roster(version: u64, at: u64, members: Vec<Member>, by: &DeviceKeys) -> Roster {
    let mut r = Roster {
        v: 1,
        team: TEAM.into(),
        name: "Acme".into(),
        version,
        at,
        members,
        spent: vec![],
        redeem: None,
        by: String::new(),
        sig: String::new(),
    };
    r.sign_with(by);
    r
}

fn pin_of(p: &Person) -> Value {
    json!({ "team": TEAM, "founder": p.account, "founder_root": p.cert.device })
}

// ---- raise_ok ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RaiseIn {
    raised: Option<String>,
    signer_kind: String,
    signer_name: String,
    browser_chained: bool,
}

fn run_raise_ok(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: RaiseIn = input(v)?;
    let raised = i.raised.as_deref().map(parse_role).transpose()?;
    let mut signer =
        Cert::new(&crate::harness::keys(1), "acct", serde_json::from_value(json!(i.signer_kind))?, &i.signer_name);
    signer.created = 0;
    Ok(json!({ "ok": raise_ok(raised, &signer, || i.browser_chained) }))
}

fn raise_ok_table() -> Example {
    let mut ex = Example::new(
        "team",
        "raise_ok",
        run_raise_ok,
        "The decision table for who may sign a paper (a roster, a presigned invite, an Ask me first invite) that \
         raises someone to a role: making no one more than a viewer, any device that approves; an editor, a \
         browser all the way back to its root (browser_chained) or the desktop app's device (from_app: an app \
         certificate, or a browser one the app named); an owner, only such a browser. Never a CLI's key, which an \
         agent on its machine can read. browser_chained is Trust::browser_chained for the signer (cert/browser_chained); \
         this table takes it as given.",
        &["arugula_e2e::team::raise_ok"],
    )
    .shapes(
        "raised: the highest role the paper makes anyone (viewer, editor, owner) or null; signer_kind and \
         signer_name: the signing device's certificate's; browser_chained: whether that device is a browser all \
         the way to its root",
        "ok: whether that device may sign it",
    );
    let signers = [
        ("browser", "Chrome on Mac"),
        ("app", "Arugula app"),
        ("browser", "Arugula app on jake-air"),
        ("cli", "terminal"),
        ("recovery", "recovery 1"),
        ("daemon", "geek"),
    ];
    for raised in [None, Some("viewer"), Some("editor"), Some("owner")] {
        for (kind, name) in signers {
            for chained in [true, false] {
                let from_app = kind == "app" || name.starts_with("Arugula app");
                let ok = match raised {
                    None | Some("viewer") => true,
                    Some("editor") => from_app || chained,
                    _ => chained,
                };
                let about = format!(
                    "raising to {}, signed by {kind} \"{name}\", {} a browser all the way to its root",
                    raised.unwrap_or("nothing"),
                    if chained { "chained as" } else { "not" }
                );
                ex.case(
                    &about,
                    json!({ "raised": raised, "signer_kind": kind, "signer_name": name, "browser_chained": chained }),
                    json!({ "ok": ok }),
                );
            }
        }
    }
    ex
}

// ---- raised ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RaisedIn {
    prev: Option<String>,
    roster: String,
    signer: String,
}

fn run_raised(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: RaisedIn = input(v)?;
    let prev: Option<Roster> = i.prev.as_deref().map(|l| ctx.get(l)).transpose()?;
    let r: Roster = ctx.get(&i.roster)?;
    Ok(json!({ "raised": role(r.raised(prev.as_ref(), &i.signer)) }))
}

fn raised() -> Example {
    let mut ex = Example::new(
        "team",
        "raised",
        run_raised,
        "The highest role a roster version makes anyone since the one before (from nothing, for a first \
         version), as raise_ok takes it: a member who wasn't there with that account and root and at least that \
         role is raised to their role (a viewer raises nothing); the signer's own entry is raised only when it \
         names another root, which counts as an owner (another identity for that account). Removing and \
         demoting raise nothing.",
        &["arugula_e2e::team::Roster::raised"],
    )
    .shapes(
        "prev: the version before's label, or null for a first version; roster: the new version's label; signer: \
         the account whose device signs it",
        "raised: owner, editor or null",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let mallory = person(&mut ex, "mallory", 3);
    let a = member(&alice, TeamRole::Owner);
    let k = &alice.keys;
    let rosters = [
        ("v1", roster(1, 1_000, vec![a.clone(), member(&bob, TeamRole::Viewer)], k)),
        ("v1-alone", roster(1, 1_000, vec![a.clone()], k)),
        ("v1-with-bob-editor", roster(1, 1_000, vec![a.clone(), member(&bob, TeamRole::Editor)], k)),
        ("v1-with-bob-owner", roster(1, 1_000, vec![a.clone(), member(&bob, TeamRole::Owner)], k)),
        ("v2-bob-editor", roster(2, 2_000, vec![a.clone(), member(&bob, TeamRole::Editor)], k)),
        ("v2-bob-owner", roster(2, 2_000, vec![a.clone(), member(&bob, TeamRole::Owner)], k)),
        (
            "v2-mallory-viewer",
            roster(2, 2_000, vec![a.clone(), member(&bob, TeamRole::Viewer), member(&mallory, TeamRole::Viewer)], k),
        ),
        (
            "v2-mallory-editor",
            roster(2, 2_000, vec![a.clone(), member(&bob, TeamRole::Viewer), member(&mallory, TeamRole::Editor)], k),
        ),
        (
            "v2-mallory-owner-bob-editor",
            roster(2, 2_000, vec![a.clone(), member(&bob, TeamRole::Editor), member(&mallory, TeamRole::Owner)], k),
        ),
        ("v2-bob-removed", roster(2, 2_000, vec![a.clone()], k)),
        (
            "v2-bob-viewer-elsewhere",
            roster(
                2,
                2_000,
                vec![a.clone(), Member { root: mallory.cert.device.clone(), ..member(&bob, TeamRole::Viewer) }],
                k,
            ),
        ),
        (
            "v2-bob-editor-elsewhere",
            roster(
                2,
                2_000,
                vec![a.clone(), Member { root: mallory.cert.device.clone(), ..member(&bob, TeamRole::Editor) }],
                k,
            ),
        ),
        (
            "v2-alice-elsewhere",
            roster(
                2,
                2_000,
                vec![Member { root: mallory.cert.device.clone(), ..a.clone() }, member(&bob, TeamRole::Viewer)],
                k,
            ),
        ),
        ("v2-alice-editor", roster(2, 2_000, vec![member(&alice, TeamRole::Editor), member(&bob, TeamRole::Owner)], k)),
    ];
    for (l, r) in &rosters {
        ex.object(l, r);
    }
    let case = |ex: &mut Example, about: &str, prev: Option<&str>, r: &str, signer: &str, want: Option<TeamRole>| {
        ex.case(about, json!({ "prev": prev, "roster": r, "signer": signer }), json!({ "raised": role(want) }));
    };
    use TeamRole::{Editor, Owner};
    case(&mut ex, "a first version with only its founder", None, "v1-alone", "alice", None);
    case(&mut ex, "a first version with a viewer", None, "v1", "alice", None);
    case(&mut ex, "a first version with an editor", None, "v1-with-bob-editor", "alice", Some(Editor));
    case(&mut ex, "a first version with a second owner", None, "v1-with-bob-owner", "alice", Some(Owner));
    case(&mut ex, "a viewer made an editor", Some("v1"), "v2-bob-editor", "alice", Some(Editor));
    case(&mut ex, "a viewer made an owner", Some("v1"), "v2-bob-owner", "alice", Some(Owner));
    case(&mut ex, "an editor made an editor again: nothing", Some("v2-bob-editor"), "v2-bob-editor", "alice", None);
    case(
        &mut ex,
        "an owner made an editor: a demotion raises nothing",
        Some("v2-bob-owner"),
        "v2-bob-editor",
        "alice",
        None,
    );
    case(&mut ex, "a viewer added", Some("v1"), "v2-mallory-viewer", "alice", None);
    case(&mut ex, "an editor added", Some("v1"), "v2-mallory-editor", "alice", Some(Editor));
    case(
        &mut ex,
        "an owner and an editor at once: the higher",
        Some("v1"),
        "v2-mallory-owner-bob-editor",
        "alice",
        Some(Owner),
    );
    case(&mut ex, "someone removed", Some("v1"), "v2-bob-removed", "alice", None);
    case(
        &mut ex,
        "a viewer's account moved to another root: still a viewer",
        Some("v1"),
        "v2-bob-viewer-elsewhere",
        "alice",
        None,
    );
    case(
        &mut ex,
        "an editor's account moved to another root: an editor's raise",
        Some("v1-with-bob-editor"),
        "v2-bob-editor-elsewhere",
        "alice",
        Some(Editor),
    );
    case(
        &mut ex,
        "the signer's own entry moved to another root: an owner's raise",
        Some("v1"),
        "v2-alice-elsewhere",
        "alice",
        Some(Owner),
    );
    case(
        &mut ex,
        "the signer's own entry demoted, another made owner",
        Some("v1"),
        "v2-alice-editor",
        "alice",
        Some(Owner),
    );
    case(
        &mut ex,
        "signed by bob, his own entry made an owner: his own entry isn't a raise",
        Some("v1"),
        "v2-bob-owner",
        "bob",
        None,
    );
    ex
}

// ---- follows ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FollowsIn {
    pin: TeamPin,
    certs: BTreeMap<String, Account>,
    prev: Option<String>,
    roster: String,
}

fn run_follows(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: FollowsIn = input(v)?;
    let certs = account_certs(ctx, &i.certs)?;
    let prev: Option<Roster> = i.prev.as_deref().map(|l| ctx.get(l)).transpose()?;
    let r: Roster = ctx.get(&i.roster)?;
    Ok(json!({ "ok": r.follows(prev.as_ref(), &i.pin, &certs) }))
}

/// An invite `by` signs, and its one-time key.
fn invite(role: TeamRole, expires: u64, one: &DeviceKeys, by: &DeviceKeys) -> Invite {
    let mut inv = Invite {
        team: TEAM.into(),
        role,
        expires,
        key: hex::encode(one.sign_public()),
        by: String::new(),
        sig: String::new(),
    };
    inv.sign_with(by);
    inv
}

/// `new` redeems `inv` with its one-time key `k`, writing the version after `prev`.
fn redeem(prev: &Roster, inv: &Invite, k: &DeviceKeys, new: Member, at: u64) -> Roster {
    let mut members = prev.members.clone();
    members.push(new.clone());
    let mut spent = prev.spent.clone();
    spent.push(Spent { key: inv.key.clone(), expires: inv.expires });
    let version = prev.version + 1;
    let proof = hex::encode(k.signature(inv.redeem_body(version, &new).as_bytes()));
    let mut r = Roster {
        v: 2,
        version,
        at,
        members,
        spent,
        redeem: Some(Redeem { invite: inv.clone(), proof }),
        ..prev.clone()
    };
    r.sign_redeem(k);
    r
}

fn follows() -> Example {
    let mut ex = Example::new(
        "team",
        "follows",
        run_follows,
        "Whether a roster version may follow the one a team daemon has (or, with none, start the team as its pin \
         says). It must be well formed (v1 without spent or redeem, or v2; ids one word of at most 120 bytes; a \
         name of 1 to 80 bytes, no control character; at least one owner), of the pinned team, newer, and signed \
         by a device trusted now of an owner of the version before (the founder for a first), one that approves \
         and may raise what this version raises (raise_ok). Or, for someone who isn't an owner, it redeems a \
         presigned invite: exactly the version before plus the invitee at the end, the invite marked spent and \
         never spent before, one version on, no earlier than the version before and no later than the invite \
         expires; the invite of this team, not for an owner, signed by an owner then who may raise to its role; \
         the one-time key's proof over exactly this member at this version; and the version signed by that key. \
         Control holds no owner's key and no one-time key, so it can add no one.",
        &["arugula_e2e::team::Roster::follows", "arugula_e2e::team::Roster::well_formed"],
    )
    .shapes(
        &format!(
            "pin: the team and its founder, as the daemon pinned them ({{ team, founder, founder_root }}); {CERTS_SHAPE}; \
             prev: the version the daemon has, by label, or null; roster: the new version, by label"
        ),
        "ok: whether the daemon takes it",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let carol = person(&mut ex, "carol", 3);
    let mallory = person(&mut ex, "mallory", 4);
    // Alice's other devices: a CLI, the app, a browser the CLI made (no
    // control would keep one now), and the app's browser from before it had
    // a kind.
    let cli = ex.key("alice-cli", 10);
    let app = ex.key("alice-app", 11);
    let laundered = ex.key("alice-browser-by-cli", 12);
    ex.object("alice-cli", &cert(&cli, "alice", Kind::Cli, "terminal", 1_100, &alice.keys));
    ex.object("alice-app", &cert(&app, "alice", Kind::App, "Arugula app", 1_200, &alice.keys));
    ex.object("alice-browser-by-cli", &cert(&laundered, "alice", Kind::Browser, "laundered", 1_300, &cli));
    // Bob's phone, revoked later.
    let phone = ex.key("bob-phone", 20);
    ex.object("bob-phone", &cert(&phone, "bob", Kind::Browser, "bob's phone", 1_100, &bob.keys));
    ex.object("bob-phone-revoked", &revoke("bob", &phone.id(), 9_000, &bob.keys));
    let pin = pin_of(&alice);
    let base = json!({
        "alice": { "certs": ["alice", "alice-cli", "alice-app", "alice-browser-by-cli"], "revocations": [] },
        "bob": { "certs": ["bob"], "revocations": [] },
        "carol": { "certs": ["carol"], "revocations": [] },
        "mallory": { "certs": ["mallory"], "revocations": [] },
    });
    let mut cases: Vec<(String, Value, Option<String>, String, bool)> = vec![];
    let mut add = |about: &str, certs: &Value, prev: Option<&str>, r: &str, ok: bool| {
        cases.push((about.into(), certs.clone(), prev.map(str::to_owned), r.into(), ok));
    };
    let (ak, bk) = (&alice.keys, &bob.keys);
    let a = member(&alice, TeamRole::Owner);
    use TeamRole::{Editor, Owner, Viewer};
    let put = |ex: &mut Example, l: &str, r: &Roster| ex.object(l, r);

    // Owners sign each version.
    let v1 = roster(1, 1_000, vec![a.clone()], ak);
    put(&mut ex, "v1", &v1);
    let v2 = roster(2, 2_000, vec![a.clone(), member(&bob, Editor)], ak);
    put(&mut ex, "v2", &v2);
    put(&mut ex, "v3-by-bob", &roster(3, 3_000, vec![a.clone(), member(&bob, Owner)], bk));
    put(&mut ex, "v3-by-mallory", &roster(3, 3_000, vec![member(&mallory, Owner)], &mallory.keys));
    put(&mut ex, "v1-by-mallory", &roster(1, 1_000, vec![member(&mallory, Owner)], &mallory.keys));
    put(&mut ex, "v2-again", &roster(2, 2_500, vec![a.clone()], ak));
    let v3 = roster(3, 3_000, vec![member(&alice, Editor), member(&bob, Owner)], ak);
    put(&mut ex, "v3-bob-owner", &v3);
    put(&mut ex, "v4-by-bob", &roster(4, 4_000, vec![member(&bob, Owner)], bk));
    put(&mut ex, "v4-by-alice-now-editor", &roster(4, 4_000, vec![member(&alice, Owner)], ak));
    put(&mut ex, "v5-skips", &roster(5, 5_000, vec![a.clone(), member(&bob, Editor)], ak));
    add("a first version, signed by the founder", &base, None, "v1", true);
    add("an owner adds an editor", &base, Some("v1"), "v2", true);
    add("an editor makes himself an owner", &base, Some("v2"), "v3-by-bob", false);
    add("an outsider writes the next version", &base, Some("v2"), "v3-by-mallory", false);
    add("an outsider (or control) starts the team over", &base, None, "v1-by-mallory", false);
    add("an older version after a newer one", &base, Some("v2"), "v1", false);
    add("another version with the same number", &base, Some("v2"), "v2-again", false);
    add("the same version again", &base, Some("v2"), "v2", false);
    add("ownership passes on: alice makes bob an owner, herself an editor", &base, Some("v2"), "v3-bob-owner", true);
    add("then bob, now an owner, signs the next", &base, Some("v3-bob-owner"), "v4-by-bob", true);
    add("and alice, now an editor, can't", &base, Some("v3-bob-owner"), "v4-by-alice-now-editor", false);
    add("a version may skip numbers", &base, Some("v2"), "v5-skips", true);

    // Tampering and form.
    let mut promoted = roster(1, 1_000, vec![a.clone(), member(&bob, Viewer)], ak);
    promoted.members[1].role = Owner;
    put(&mut ex, "v1-promoted-after-signing", &promoted);
    let mut other_team = roster(1, 1_000, vec![a.clone()], ak);
    other_team.team = "t2".into();
    other_team.sign_with(ak);
    put(&mut ex, "v1-of-team-t2", &other_team);
    put(&mut ex, "v2-no-owner", &roster(2, 2_000, vec![member(&alice, Editor)], ak));
    let mut bad_name = roster(2, 2_000, vec![a.clone()], ak);
    bad_name.name = "Acme\nversion 99".into();
    bad_name.sign_with(ak);
    put(&mut ex, "v2-newline-in-name", &bad_name);
    let mut long_name = roster(2, 2_000, vec![a.clone()], ak);
    long_name.name = "a".repeat(81);
    long_name.sign_with(ak);
    put(&mut ex, "v2-name-81", &long_name);
    let mut v3_format = roster(2, 2_000, vec![a.clone()], ak);
    v3_format.v = 3;
    v3_format.sign_with(ak);
    put(&mut ex, "v2-format-v3", &v3_format);
    let mut v1_spent = roster(2, 2_000, vec![a.clone()], ak);
    v1_spent.spent = vec![Spent { key: "ab".repeat(32), expires: 9 }];
    v1_spent.sign_with(ak);
    put(&mut ex, "v2-format-v1-with-spent", &v1_spent);
    let mut v2_spent = v1_spent.clone();
    v2_spent.v = 2;
    v2_spent.sign_with(ak);
    put(&mut ex, "v2-format-v2-with-spent", &v2_spent);
    let mut space_member = roster(2, 2_000, vec![a.clone(), member(&bob, Viewer)], ak);
    space_member.members[1].name = "bob login".into();
    space_member.sign_with(ak);
    put(&mut ex, "v2-space-in-member-name", &space_member);
    add("a role changed after the founder signed", &base, None, "v1-promoted-after-signing", false);
    add("another team's roster", &base, None, "v1-of-team-t2", false);
    add("a version with no owner", &base, Some("v1"), "v2-no-owner", false);
    add("a newline in the team's name", &base, Some("v1"), "v2-newline-in-name", false);
    add("a team name of 81 bytes", &base, Some("v1"), "v2-name-81", false);
    add("a roster of format 3", &base, Some("v1"), "v2-format-v3", false);
    add("a v1 roster carrying spent invites (v2's)", &base, Some("v1"), "v2-format-v1-with-spent", false);
    add("a v2 roster an owner signs, carrying spent invites", &base, Some("v1"), "v2-format-v2-with-spent", true);
    add("a space in a member's name", &base, Some("v1"), "v2-space-in-member-name", false);
    let mut moved = base.clone();
    moved["alice"] = json!({ "certs": ["mallory"], "revocations": [] });
    add("control hands out another root's certificates for the founder", &moved, None, "v1", false);

    // Which of an owner's devices may raise whom (raise_ok).
    let v1b = roster(1, 1_000, vec![a.clone(), member(&bob, Viewer)], ak);
    put(&mut ex, "v1-bob-viewer", &v1b);
    let signed = |members: Vec<Member>, k: &DeviceKeys| roster(2, 2_000, members, k);
    for (what, members) in [
        ("makes mallory an owner", vec![a.clone(), member(&bob, Viewer), member(&mallory, Owner)]),
        ("makes mallory an editor", vec![a.clone(), member(&bob, Viewer), member(&mallory, Editor)]),
        ("makes bob an editor", vec![a.clone(), member(&bob, Editor)]),
        ("adds mallory as a viewer", vec![a.clone(), member(&bob, Viewer), member(&mallory, Viewer)]),
        ("takes bob out", vec![a.clone()]),
        (
            "moves alice's own entry to another root",
            vec![Member { root: mallory.cert.device.clone(), ..a.clone() }, member(&bob, Viewer)],
        ),
    ] {
        let raises = what.contains("owner") || what.contains("editor") || what.contains("own entry");
        let editor_only = what.contains("editor");
        for (dev, k, ok) in [
            ("alice's browser", ak, true),
            ("alice's CLI", &cli, !raises),
            ("alice's app", &app, !raises || editor_only),
            ("a browser alice's CLI made: never trusted", &laundered, false),
        ] {
            let label = format!(
                "v2-{}-by-{}",
                what.replace(' ', "-").replace('\'', ""),
                dev.replace(' ', "-").replace('\'', "")
            );
            put(&mut ex, &label, &signed(members.clone(), k));
            add(&format!("{dev} {what}"), &base, Some("v1-bob-viewer"), &label, ok);
        }
    }
    put(&mut ex, "v1-founded-by-app", &roster(1, 1_000, vec![a.clone()], &app));
    put(&mut ex, "v1-founded-by-cli", &roster(1, 1_000, vec![a.clone()], &cli));
    put(&mut ex, "v1-founded-by-cli-with-editor", &roster(1, 1_000, vec![a.clone(), member(&bob, Editor)], &cli));
    put(&mut ex, "v1-founded-by-app-with-editor", &roster(1, 1_000, vec![a.clone(), member(&bob, Editor)], &app));
    put(&mut ex, "v1-founded-by-app-with-owner", &roster(1, 1_000, vec![a.clone(), member(&bob, Owner)], &app));
    add("the app founds a team with only its owner", &base, None, "v1-founded-by-app", true);
    add("a CLI founds a team with only its owner", &base, None, "v1-founded-by-cli", true);
    add("a CLI founds a team with an editor in it", &base, None, "v1-founded-by-cli-with-editor", false);
    add("the app founds a team with an editor in it", &base, None, "v1-founded-by-app-with-editor", true);
    add("the app founds a team with a second owner", &base, None, "v1-founded-by-app-with-owner", false);
    let mut bob_editor = v1b.clone();
    bob_editor.members[1].role = Editor;
    bob_editor.sign_with(ak);
    put(&mut ex, "v1-bob-editor", &bob_editor);
    put(
        &mut ex,
        "v2-bob-editor-elsewhere-by-cli",
        &signed(vec![a.clone(), Member { root: mallory.cert.device.clone(), ..member(&bob, Editor) }], &cli),
    );
    add(
        "a CLI swaps an editor's account for another root",
        &base,
        Some("v1-bob-editor"),
        "v2-bob-editor-elsewhere-by-cli",
        false,
    );
    let mut alice_revoked = base.clone();
    alice_revoked["alice"]["revocations"] = json!(["alice-cli-revoked"]);
    ex.object("alice-cli-revoked", &revoke("alice", &cli.id(), 1_500, ak));
    add(
        "a viewer added by alice's CLI, after she revoked it",
        &alice_revoked,
        Some("v1-bob-viewer"),
        "v2-adds-mallory-as-a-viewer-by-alices-CLI",
        false,
    );

    // Presigned invites.
    let one = |ex: &mut Example, l: &str, n: u8| ex.key(l, n);
    let k1 = one(&mut ex, "one-time-1", 30);
    let inv = invite(Editor, 10_000, &k1, ak);
    let v2r = redeem(&v1, &inv, &k1, member(&bob, Editor), 2_000);
    put(&mut ex, "v2-bob-redeems", &v2r);
    put(&mut ex, "v3-carol-redeems-the-same-invite", &redeem(&v2r, &inv, &k1, member(&carol, Editor), 3_000));
    let mut v3o = v2r.clone();
    v3o.version = 3;
    v3o.at = 3_000;
    v3o.redeem = None;
    v3o.sign_with(ak);
    put(&mut ex, "v3-by-alice-after-redeem", &v3o);
    let k2 = one(&mut ex, "one-time-2", 31);
    let inv2 = invite(Viewer, 10_000, &k2, ak);
    put(&mut ex, "v4-carol-redeems-a-second-invite", &redeem(&v3o, &inv2, &k2, member(&carol, Viewer), 4_000));
    add("bob adds himself with alice's presigned invite", &base, Some("v1"), "v2-bob-redeems", true);
    add(
        "carol uses the same invite after him",
        &base,
        Some("v2-bob-redeems"),
        "v3-carol-redeems-the-same-invite",
        false,
    );
    add(
        "alice's next version carries the spent invite on",
        &base,
        Some("v2-bob-redeems"),
        "v3-by-alice-after-redeem",
        true,
    );
    add(
        "a second invite works the same way",
        &base,
        Some("v3-by-alice-after-redeem"),
        "v4-carol-redeems-a-second-invite",
        true,
    );
    let k3 = one(&mut ex, "one-time-3", 32);
    let vinv = invite(Viewer, 10_000, &k3, ak);
    let good = redeem(&v1, &vinv, &k3, member(&bob, Viewer), 2_000);
    put(&mut ex, "v2-bob-redeems-viewer", &good);
    add("bob redeems a viewer's invite", &base, Some("v1"), "v2-bob-redeems-viewer", true);
    let mut stolen = good.clone();
    stolen.members[1] = member(&mallory, Viewer);
    stolen.sign_with(&mallory.keys);
    put(&mut ex, "v2-bobs-proof-moved-to-mallory", &stolen);
    add("control moves bob's proof to mallory", &base, Some("v1"), "v2-bobs-proof-moved-to-mallory", false);
    let mut stolen_k = good.clone();
    stolen_k.members[1] = member(&mallory, Viewer);
    stolen_k.sign_redeem(&k3);
    put(&mut ex, "v2-bobs-proof-for-mallory-signed-by-the-key", &stolen_k);
    add(
        "bob's proof, with mallory in his place, the version signed with the one-time key",
        &base,
        Some("v1"),
        "v2-bobs-proof-for-mallory-signed-by-the-key",
        false,
    );
    let forger = one(&mut ex, "forger", 40);
    put(&mut ex, "v2-forged-proof", &redeem(&v1, &vinv, &forger, member(&mallory, Viewer), 2_000));
    add("a proof without the one-time key", &base, Some("v1"), "v2-forged-proof", false);
    let mut higher = good.clone();
    higher.members[1].role = Editor;
    higher.sign_redeem(&k3);
    put(&mut ex, "v2-redeem-higher-role", &higher);
    add("a higher role than the invite's", &base, Some("v1"), "v2-redeem-higher-role", false);
    let v2m = roster(2, 2_000, vec![a.clone(), member(&mallory, Viewer)], ak);
    put(&mut ex, "v2-mallory-viewer", &v2m);
    let mut changed = redeem(&v2m, &vinv, &k3, member(&bob, Viewer), 3_000);
    changed.members[1].role = Editor;
    changed.sign_redeem(&k3);
    put(&mut ex, "v3-redeem-promotes-another-member", &changed);
    add(
        "a redeem that also promotes another member",
        &base,
        Some("v2-mallory-viewer"),
        "v3-redeem-promotes-another-member",
        false,
    );
    put(&mut ex, "v3-redeem-unchanged", &redeem(&v2m, &vinv, &k3, member(&bob, Viewer), 3_000));
    add("the same redeem with nothing else changed", &base, Some("v2-mallory-viewer"), "v3-redeem-unchanged", true);
    let mut late = good.clone();
    late.at = 10_001;
    late.sign_redeem(&k3);
    put(&mut ex, "v2-redeem-after-expiry", &late);
    add("redeemed 1 ms after the invite expires", &base, Some("v1"), "v2-redeem-after-expiry", false);
    let mut on_time = good.clone();
    on_time.at = 10_000;
    on_time.sign_redeem(&k3);
    put(&mut ex, "v2-redeem-at-expiry", &on_time);
    add("redeemed the very ms the invite expires", &base, Some("v1"), "v2-redeem-at-expiry", true);
    let v1_later = roster(1, 5_000, vec![a.clone()], ak);
    put(&mut ex, "v1-at-5000", &v1_later);
    put(&mut ex, "v2-redeem-backdated", &redeem(&v1_later, &vinv, &k3, member(&bob, Viewer), 4_999));
    add("redeemed backdated, before the version it follows", &base, Some("v1-at-5000"), "v2-redeem-backdated", false);
    let mut by_device = good.clone();
    by_device.sign_with(bk);
    put(&mut ex, "v2-redeem-signed-by-bobs-device", &by_device);
    add(
        "the redeeming version signed by bob's device, not the one-time key",
        &base,
        Some("v1"),
        "v2-redeem-signed-by-bobs-device",
        false,
    );
    let k4 = one(&mut ex, "one-time-4", 33);
    put(
        &mut ex,
        "v3-redeem-invite-by-an-editor",
        &redeem(&v2, &invite(Viewer, 10_000, &k4, bk), &k4, member(&mallory, Viewer), 3_000),
    );
    add("an invite an editor signed", &base, Some("v2"), "v3-redeem-invite-by-an-editor", false);
    let k5 = one(&mut ex, "one-time-5", 34);
    put(
        &mut ex,
        "v2-redeem-owner-invite",
        &redeem(&v1, &invite(Owner, 10_000, &k5, ak), &k5, member(&bob, Owner), 2_000),
    );
    add("an invite to be an owner: never presigned", &base, Some("v1"), "v2-redeem-owner-invite", false);
    let mut unspent = good.clone();
    unspent.spent.clear();
    unspent.sign_redeem(&k3);
    put(&mut ex, "v2-redeem-not-marked-spent", &unspent);
    add("a redeem that doesn't mark its invite spent", &base, Some("v1"), "v2-redeem-not-marked-spent", false);
    let mut owner_redeem = good.clone();
    owner_redeem.sign_with(ak);
    put(&mut ex, "v2-redeem-signed-by-an-owner", &owner_redeem);
    add("an owner's version carrying a redeem", &base, Some("v1"), "v2-redeem-signed-by-an-owner", false);
    let mut old = good.clone();
    old.v = 1;
    old.sign_redeem(&k3);
    put(&mut ex, "v2-redeem-in-format-v1", &old);
    add("a v1 roster carrying a redeem", &base, Some("v1"), "v2-redeem-in-format-v1", false);
    let k6 = one(&mut ex, "one-time-6", 35);
    let mut elsewhere = invite(Viewer, 10_000, &k6, ak);
    elsewhere.team = "t2".into();
    elsewhere.sign_with(ak);
    put(&mut ex, "v2-redeem-another-teams-invite", &redeem(&v1, &elsewhere, &k6, member(&bob, Viewer), 2_000));
    add("another team's invite", &base, Some("v1"), "v2-redeem-another-teams-invite", false);
    let mut jump = good.clone();
    jump.version = 3;
    let proof = hex::encode(k3.signature(vinv.redeem_body(3, &jump.members[1]).as_bytes()));
    jump.redeem = Some(Redeem { invite: vinv.clone(), proof });
    jump.sign_redeem(&k3);
    put(&mut ex, "v3-redeem-skips-a-version", &jump);
    add("a redeem two versions on", &base, Some("v1"), "v3-redeem-skips-a-version", false);
    put(
        &mut ex,
        "v2-redeem-alice-again",
        &redeem(&v1, &vinv, &k3, Member { name: "alice2".into(), ..member(&alice, Viewer) }, 2_000),
    );
    add("a redeem adding someone already in the team", &base, Some("v1"), "v2-redeem-alice-again", false);
    let mut renamed = good.clone();
    renamed.name = "Evil Corp".into();
    renamed.sign_redeem(&k3);
    put(&mut ex, "v2-redeem-renames-the-team", &renamed);
    add("a redeem that renames the team", &base, Some("v1"), "v2-redeem-renames-the-team", false);
    for (what, by, l, n, role, ok) in [
        ("an editor's invite alice's CLI signed", &cli, "one-time-cli-editor", 36u8, Editor, false),
        ("a viewer's invite alice's CLI signed", &cli, "one-time-cli-viewer", 37, Viewer, true),
        ("an editor's invite alice's app signed", &app, "one-time-app-editor", 38, Editor, true),
        (
            "an editor's invite a browser alice's CLI made signed",
            &laundered,
            "one-time-laundered-editor",
            39,
            Editor,
            false,
        ),
    ] {
        let k = one(&mut ex, l, n);
        let label = format!("v2-redeem-{l}");
        put(&mut ex, &label, &redeem(&v1, &invite(role, 10_000, &k, by), &k, member(&mallory, role), 2_000));
        add(what, &base, Some("v1"), &label, ok);
    }
    // Bob joins from his phone, is removed, then takes the phone off his
    // account: a daemon checking the team from the start still gets there.
    let mut v3_removed = roster(3, 3_000, vec![a.clone()], ak);
    v3_removed.v = 2;
    v3_removed.spent = v2r.spent.clone();
    v3_removed.sign_with(ak);
    put(&mut ex, "v3-bob-removed", &v3_removed);
    let mut phone_gone = base.clone();
    phone_gone["bob"] = json!({ "certs": ["bob", "bob-phone"], "revocations": ["bob-phone-revoked"] });
    add(
        "after bob revokes the phone he joined from: his redeem still checks",
        &phone_gone,
        Some("v1"),
        "v2-bob-redeems",
        true,
    );
    add("and so does his removal", &phone_gone, Some("v2-bob-redeems"), "v3-bob-removed", true);

    for (about, certs, prev, r, ok) in cases {
        ex.case(&about, json!({ "pin": pin, "certs": certs, "prev": prev, "roster": r }), json!({ "ok": ok }));
    }
    ex
}

// ---- bodies ----

#[derive(Deserialize)]
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
enum BodyIn {
    Roster { roster: String },
    Invite { invite: String },
    Redeem { invite: String, version: u64, member: Member },
    AskInvite { invite: String },
    Ask { invite: String, account: String, root: String },
    Join { pin: TeamPin, daemon: String },
    Move { daemon: String, team: Option<TeamPin>, at: u64 },
}

fn run_bodies(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let body = match input::<BodyIn>(v)? {
        BodyIn::Roster { roster } => ctx.get::<Roster>(&roster)?.body(),
        BodyIn::Invite { invite } => ctx.get::<Invite>(&invite)?.body(),
        BodyIn::Redeem { invite, version, member } => ctx.get::<Invite>(&invite)?.redeem_body(version, &member),
        BodyIn::AskInvite { invite } => ctx.get::<AskInvite>(&invite)?.body(),
        BodyIn::Ask { invite, account, root } => ctx.get::<AskInvite>(&invite)?.ask_body(&account, &root),
        BodyIn::Join { pin, daemon } => pin.join_body(&daemon),
        BodyIn::Move { daemon, team, at } => Move::body(&daemon, team.as_ref(), at),
    };
    Ok(json!({ "body": body }))
}

fn bodies() -> Example {
    let mut ex = Example::new(
        "team",
        "bodies",
        run_bodies,
        "The exact bytes each team paper's signature is over. A roster's sig is its by's over its body (by is a \
         device id, or for a redeeming version the one-time key, hex); an invite's, the owner's device's; a \
         redeem's proof, the one-time key's over the redeem body; an Ask me first invite's, the owner's device's, \
         and its proof, the one-time key's over the ask body; a join's, the approving device's; a move's, a \
         device of the machine's own account. v1 rosters carry no spent or redeem lines, so older daemons check \
         them. The first lines are frozen (#504).",
        &[
            "arugula_e2e::team::Roster::body",
            "arugula_e2e::team::Invite::body",
            "arugula_e2e::team::Invite::redeem_body",
            "arugula_e2e::team::AskInvite::body",
            "arugula_e2e::team::AskInvite::ask_body",
            "arugula_e2e::team::TeamPin::join_body",
            "arugula_e2e::team::Move::body",
        ],
    )
    .shapes(
        "of: which paper (roster, invite, redeem, ask_invite, ask, join, move), and what it needs: a roster's or \
         an invite's label; redeem: the invite, the version and the member; ask: the invite, the account and its \
         root; join: the pin and the daemon's id; move: the daemon's id, the team it moves into (a pin, or null for \
         its own account) and at",
        "body: the signed text",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let k = ex.key("one-time", 30);
    let inv = invite(TeamRole::Editor, 10_000, &k, &alice.keys);
    ex.object("invite", &inv);
    let v1 = roster(1, 1_000, vec![member(&alice, TeamRole::Owner)], &alice.keys);
    ex.object("v1", &v1);
    ex.object("v2-redeemed", &redeem(&v1, &inv, &k, member(&bob, TeamRole::Editor), 2_000));
    let mut ask = AskInvite {
        team: TEAM.into(),
        role: TeamRole::Owner,
        made: 1_000,
        expires: 2_000,
        key: hex::encode(k.sign_public()),
        by: String::new(),
        sig: String::new(),
    };
    ask.sign_with(&alice.keys);
    ex.object("ask-invite", &ask);
    let pin: TeamPin = serde_json::from_value(pin_of(&alice)).unwrap();
    ex.computed("a v1 roster", json!({ "of": "roster", "roster": "v1" }));
    ex.computed(
        "a v2 roster an invitee wrote: spent and redeem lines, by the one-time key",
        json!({ "of": "roster", "roster": "v2-redeemed" }),
    );
    ex.computed("a presigned invite", json!({ "of": "invite", "invite": "invite" }));
    ex.computed(
        "what the one-time key signs for bob at version 2",
        json!({ "of": "redeem", "invite": "invite", "version": 2, "member": member(&bob, TeamRole::Editor) }),
    );
    ex.computed("an Ask me first invite", json!({ "of": "ask_invite", "invite": "ask-invite" }));
    ex.computed(
        "what its one-time key signs for bob",
        json!({ "of": "ask", "invite": "ask-invite", "account": "bob", "root": bob.cert.device }),
    );
    ex.computed("a daemon's join into the team", json!({ "of": "join", "pin": pin, "daemon": "d30b9802b6365451" }));
    ex.computed(
        "a machine moved into the team",
        json!({ "of": "move", "daemon": "d30b9802b6365451", "team": pin, "at": 5_000 }),
    );
    ex.computed(
        "a machine moved back to its account",
        json!({ "of": "move", "daemon": "d30b9802b6365451", "team": null, "at": 6_000 }),
    );
    ex
}

// ---- ask invites ----

#[derive(Deserialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
enum AskIn {
    SignedBy { invite: String, by: String },
    Proves { invite: String, account: String, root: String, proof: String },
    Raised { invite: String },
}

fn run_ask(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(match input::<AskIn>(v)? {
        AskIn::SignedBy { invite, by } => json!({ "ok": ctx.get::<AskInvite>(&invite)?.signed_by(&ctx.get(&by)?) }),
        AskIn::Proves { invite, account, root, proof } => {
            json!({ "ok": ctx.get::<AskInvite>(&invite)?.proves(&account, &root, &proof) })
        }
        AskIn::Raised { invite } => json!({ "raised": role(ctx.get::<AskInvite>(&invite)?.raised()) }),
    })
}

fn ask_invite() -> Example {
    let mut ex = Example::new(
        "team",
        "ask_invite",
        run_ask,
        "An Ask me first invite (#762): an owner's device signs it, naming a one-time key whose private half is \
         only in the link; whoever uses the link proves they hold it by signing their account and root with that \
         key, and an owner then admits them with the next roster. signed_by: by its by, a device that approves \
         (one that raises also needs a browser all the way to the root, as raise_ok says: that's checked beside \
         it). proves: the one-time key's signature over exactly that account and root, so control can't move a \
         proof to an account of its own. Its domain is its own, so it never reads as a presigned invite.",
        &[
            "arugula_e2e::team::AskInvite::signed_by",
            "arugula_e2e::team::AskInvite::proves",
            "arugula_e2e::team::AskInvite::raised",
        ],
    )
    .shapes(
        "check: signed_by (invite, by: a certificate's label), proves (invite, account, root, proof) or raised \
         (invite)",
        "signed_by and proves: ok; raised: the role it raises someone to, or null for a viewer's",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let mallory = person(&mut ex, "mallory", 3);
    let machine = ex.key("alice-box", 4);
    ex.object("alice-box", &cert(&machine, "alice", Kind::Daemon, "box", 1_100, &alice.keys));
    let k = ex.key("one-time", 30);
    let make = |role, by: &DeviceKeys| {
        let mut a = AskInvite {
            team: TEAM.into(),
            role,
            made: 1_000,
            expires: 2_000,
            key: hex::encode(k.sign_public()),
            by: String::new(),
            sig: String::new(),
        };
        a.sign_with(by);
        a
    };
    let inv = make(TeamRole::Owner, &alice.keys);
    ex.object("owner-invite", &inv);
    ex.object("viewer-invite", &make(TeamRole::Viewer, &alice.keys));
    ex.object("editor-invite", &make(TeamRole::Editor, &alice.keys));
    ex.object("invite-by-box", &make(TeamRole::Viewer, &machine));
    let mut bent = inv.clone();
    bent.expires = 99_000;
    ex.object("owner-invite-expiry-changed", &bent);
    let mut as_presigned = inv.clone();
    as_presigned.role = TeamRole::Editor;
    ex.object("owner-invite-role-changed", &as_presigned);
    let proof = hex::encode(k.signature(inv.ask_body("bob", &bob.cert.device).as_bytes()));
    let forged = hex::encode(mallory.keys.signature(inv.ask_body("mallory", &mallory.cert.device).as_bytes()));
    let signed = |ex: &mut Example, about: &str, invite: &str, by: &str, ok: bool| {
        ex.case(about, json!({ "check": "signed_by", "invite": invite, "by": by }), json!({ "ok": ok }));
    };
    signed(&mut ex, "alice's invite, by her device", "owner-invite", "alice", true);
    signed(&mut ex, "alice's invite, said to be by bob's device", "owner-invite", "bob", false);
    signed(&mut ex, "its expiry changed after she signed", "owner-invite-expiry-changed", "alice", false);
    signed(&mut ex, "its role changed after she signed", "owner-invite-role-changed", "alice", false);
    signed(&mut ex, "signed by a machine of hers: a machine approves nothing", "invite-by-box", "alice-box", false);
    let proves = |ex: &mut Example, about: &str, account: &str, root: &str, p: &str, ok: bool| {
        ex.case(
            about,
            json!({ "check": "proves", "invite": "owner-invite", "account": account, "root": root, "proof": p }),
            json!({ "ok": ok }),
        );
    };
    proves(&mut ex, "bob's proof, for bob and his root", "bob", &bob.cert.device, &proof, true);
    proves(&mut ex, "bob's proof, moved to mallory", "mallory", &mallory.cert.device, &proof, false);
    proves(&mut ex, "bob's proof, moved to another root of bob's", "bob", &mallory.cert.device, &proof, false);
    proves(&mut ex, "mallory's proof signed without the link's key", "mallory", &mallory.cert.device, &forged, false);
    for (l, want) in
        [("owner-invite", Some(TeamRole::Owner)), ("editor-invite", Some(TeamRole::Editor)), ("viewer-invite", None)]
    {
        ex.case(
            &format!("what {l} raises to"),
            json!({ "check": "raised", "invite": l }),
            json!({ "raised": role(want) }),
        );
    }
    ex
}

// ---- join ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinIn {
    pin: TeamPin,
    daemon: String,
    approver: String,
    sig: String,
}

fn run_join(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: JoinIn = input(v)?;
    Ok(json!({ "ok": i.pin.join_signed_by(&i.daemon, &ctx.get(&i.approver)?, &i.sig) }))
}

fn join() -> Example {
    let mut ex = Example::new(
        "team",
        "join",
        run_join,
        "A daemon joining straight into a team (#100): the approving device signs which team and founder it pins, \
         so a daemon pins only a team a device of the approving account chose, never one control adds. Checked \
         against the approver's certificate (that it's trusted is the daemon's other check): a device that \
         approves, whose key signed the join body for this daemon.",
        &["arugula_e2e::team::TeamPin::join_signed_by"],
    )
    .shapes("pin: { team, founder, founder_root }; daemon: its id; approver: a certificate's label; sig: hex", "ok");
    let alice = person(&mut ex, "alice", 1);
    let mallory = person(&mut ex, "mallory", 2);
    let machine = ex.key("alice-box", 3);
    ex.object("alice-box", &cert(&machine, "alice", Kind::Daemon, "box", 1_100, &alice.keys));
    let pin = pin_of(&alice);
    let p: TeamPin = serde_json::from_value(pin.clone()).unwrap();
    let daemon = "d30b9802b6365451";
    let sig = hex::encode(alice.keys.signature(p.join_body(daemon).as_bytes()));
    let box_sig = hex::encode(machine.signature(p.join_body(daemon).as_bytes()));
    let swapped = json!({ "team": TEAM, "founder": "alice", "founder_root": mallory.cert.device });
    let case = |ex: &mut Example, about: &str, pin: &Value, d: &str, by: &str, s: &str, ok: bool| {
        ex.case(about, json!({ "pin": pin, "daemon": d, "approver": by, "sig": s }), json!({ "ok": ok }));
    };
    case(&mut ex, "alice puts the daemon in her team", &pin, daemon, "alice", &sig, true);
    case(&mut ex, "the same signature for another daemon", &pin, "0123456789abcdef", "alice", &sig, false);
    case(&mut ex, "with the founder's root swapped", &swapped, daemon, "alice", &sig, false);
    case(&mut ex, "said to be by mallory", &pin, daemon, "mallory", &sig, false);
    case(&mut ex, "signed by a machine: it approves nothing", &pin, daemon, "alice-box", &box_sig, false);
    ex
}

// ---- moves ----

#[derive(Deserialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
enum MoveIn {
    SignedFor { r#move: String, daemon: String, by: String },
    OwnerTakesOut { r#move: String, daemon: String, roster: String, certs: BTreeMap<String, Account> },
}

fn run_move(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    Ok(match input::<MoveIn>(v)? {
        MoveIn::SignedFor { r#move, daemon, by } => {
            json!({ "ok": ctx.get::<Move>(&r#move)?.signed_for(&daemon, &ctx.get(&by)?) })
        }
        MoveIn::OwnerTakesOut { r#move, daemon, roster, certs } => {
            let certs = account_certs(ctx, &certs)?;
            json!({ "ok": ctx.get::<Move>(&r#move)?.owner_takes_out(&daemon, &ctx.get(&roster)?, &certs) })
        }
    })
}

fn moves() -> Example {
    let mut ex = Example::new(
        "team",
        "move",
        run_move,
        "A joined machine moved into a team, between teams, or back to its account: signed for that daemon and \
         that time by a device that approves, so control can neither make one up nor replay an old one (the \
         daemon takes only a move newer than its last). signed_for: by the certificate named (trusted is the \
         daemon's other check). owner_takes_out (#332): one of the team's owners, by a device trusted now from \
         the root the roster lists, taking a machine out of the team back to its own account; never into a team.",
        &["arugula_e2e::team::Move::signed_for", "arugula_e2e::team::Move::owner_takes_out"],
    )
    .shapes(
        &format!(
            "check: signed_for (move, daemon, by: a certificate's label) or owner_takes_out (move, daemon, roster: \
             a label, {CERTS_SHAPE})"
        ),
        "ok",
    );
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let mallory = person(&mut ex, "mallory", 3);
    let pin: TeamPin = serde_json::from_value(pin_of(&alice)).unwrap();
    let daemon = "d30b9802b6365451";
    let sign = |who: &Person, team: Option<&TeamPin>, at| Move {
        team: team.cloned(),
        at,
        by: who.cert.device.clone(),
        sig: hex::encode(who.keys.signature(Move::body(daemon, team, at).as_bytes())),
    };
    let into = sign(&alice, Some(&pin), 5_000);
    ex.object("into-by-alice", &into);
    ex.object("into-team-dropped", &Move { team: None, ..into.clone() });
    ex.object("into-time-changed", &Move { at: 5_001, ..into.clone() });
    ex.object("out-by-alice", &sign(&alice, None, 7_000));
    ex.object("out-by-bob", &sign(&bob, None, 7_000));
    ex.object("out-by-mallory", &sign(&mallory, None, 7_000));
    ex.object(
        "roster",
        &roster(1, 1_000, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Editor)], &alice.keys),
    );
    let s = |ex: &mut Example, about: &str, m: &str, d: &str, by: &str, ok: bool| {
        ex.case(about, json!({ "check": "signed_for", "move": m, "daemon": d, "by": by }), json!({ "ok": ok }));
    };
    s(&mut ex, "alice moves her machine into the team", "into-by-alice", daemon, "alice", true);
    s(&mut ex, "the same move for another daemon", "into-by-alice", "0123456789abcdef", "alice", false);
    s(&mut ex, "said to be by mallory", "into-by-alice", daemon, "mallory", false);
    s(&mut ex, "control drops the team from it", "into-team-dropped", daemon, "alice", false);
    s(&mut ex, "control changes its time", "into-time-changed", daemon, "alice", false);
    s(&mut ex, "alice moves it back to her account", "out-by-alice", daemon, "alice", true);
    let certs =
        json!({ "alice": { "certs": ["alice"] }, "bob": { "certs": ["bob"] }, "mallory": { "certs": ["mallory"] } });
    let o = |ex: &mut Example, about: &str, m: &str, ok: bool| {
        ex.case(
            about,
            json!({ "check": "owner_takes_out", "move": m, "daemon": daemon, "roster": "roster", "certs": certs }),
            json!({ "ok": ok }),
        );
    };
    o(&mut ex, "the team's owner takes bob's machine out", "out-by-alice", true);
    o(&mut ex, "an editor can't", "out-by-bob", false);
    o(&mut ex, "nor an outsider", "out-by-mallory", false);
    o(&mut ex, "nor may the owner move it into a team this way", "into-by-alice", false);
    ex.case(
        "the owner's move, with control handing out another root's certificates for her",
        json!({ "check": "owner_takes_out", "move": "out-by-alice", "daemon": daemon, "roster": "roster",
                "certs": { "alice": { "certs": ["mallory"] } } }),
        json!({ "ok": false }),
    );
    ex
}

// ---- devices ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DevicesIn {
    roster: String,
    account: String,
    certs: BTreeMap<String, Account>,
}

fn run_devices(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: DevicesIn = input(v)?;
    let certs = account_certs(ctx, &i.certs)?;
    let mut d: Vec<String> = ctx.get::<Roster>(&i.roster)?.devices(&i.account, &certs).devices.into_keys().collect();
    d.sort();
    Ok(json!({ "devices": d }))
}

fn devices() -> Example {
    let mut ex = Example::new(
        "team",
        "devices",
        run_devices,
        "The devices a member account trusts now, from the root the roster lists for it (Trust::evaluate, \
         cert/evaluate): another root's certificates from control count for nothing, and an account that isn't \
         a member has none.",
        &["arugula_e2e::team::Roster::devices"],
    )
    .shapes(&format!("roster: a label; account; {CERTS_SHAPE}"), "devices: their ids, sorted");
    let alice = person(&mut ex, "alice", 1);
    let bob = person(&mut ex, "bob", 2);
    let other = ex.key("another-bob", 3);
    ex.object("another-bob", &cert(&other, "bob", Kind::Browser, "bob's laptop", 1_000, &other));
    let phone = ex.key("bob-phone", 4);
    ex.object("bob-phone", &cert(&phone, "bob", Kind::Browser, "bob's phone", 1_100, &bob.keys));
    ex.object("bob-phone-revoked", &revoke("bob", &phone.id(), 2_000, &bob.keys));
    ex.object(
        "roster",
        &roster(1, 1_000, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Viewer)], &alice.keys),
    );
    let case = |ex: &mut Example, about: &str, account: &str, certs: Value, want: Vec<String>| {
        ex.case(about, json!({ "roster": "roster", "account": account, "certs": certs }), json!({ "devices": want }));
    };
    let mut both = vec![bob.cert.device.clone(), phone.id()];
    both.sort();
    case(&mut ex, "bob's root and his phone", "bob", json!({ "bob": { "certs": ["bob", "bob-phone"] } }), both);
    case(
        &mut ex,
        "his phone revoked",
        "bob",
        json!({ "bob": { "certs": ["bob", "bob-phone"], "revocations": ["bob-phone-revoked"] } }),
        vec![bob.cert.device.clone()],
    );
    case(&mut ex, "another root's certificates for bob", "bob", json!({ "bob": { "certs": ["another-bob"] } }), vec![]);
    case(&mut ex, "an account not in the roster", "mallory", json!({ "mallory": { "certs": ["alice"] } }), vec![]);
    case(&mut ex, "a member control sent nothing for", "alice", json!({}), vec![]);
    ex
}
