//! DK7 (#388, aru-126): a team's owners change only with the passkey of an
//! owner of the version before, over the new version's bytes, whichever of
//! that owner's devices signs it; editors and viewers change as before.
//! Over HTTP, against control's own passkey log.

use arugula_e2e::{
    DeviceKeys, Kind,
    cert::Cert,
    now_ms, passkey,
    team::{OwnerProof, Roster, TeamRole},
};
use serde_json::{Value, json};

use crate::routing_wire::{Control, control, daemon, member, person, roster, session};

const TEAM: &str = "0123456789abcdef";

/// A passkey `id` of `account`, made for control's own host (the test
/// server's), its key from `seed`.
fn site_passkey(c: &Control, account: &str, id: &str, seed: u8) -> ed25519_dalek::SigningKey {
    let k = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
    let host = c.base.trim_start_matches("http://").split(':').next().unwrap().to_owned();
    let d = passkey::Draft {
        account: account.into(),
        event: passkey::Change::Added,
        credential: id.into(),
        rp: host,
        alg: passkey::EDDSA,
        key: k.verifying_key().to_bytes().to_vec(),
        aaguid: passkey::NO_AAGUID.into(),
        at: 1,
        by: "signup".into(),
    };
    c.app.db.add_passkey_logged(&d.key.clone(), d.after(None), false).unwrap().unwrap();
    k
}

/// Passkey `id` (key `k`)'s assertion over `challenge`, from the test
/// server's page, with UP and UV.
fn assertion(c: &Control, id: &str, k: &ed25519_dalek::SigningKey, challenge: &str) -> Value {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    use ed25519_dalek::Signer as _;
    use sha2::{Digest as _, Sha256};
    let host = c.base.trim_start_matches("http://").split(':').next().unwrap();
    let cd = serde_json::to_vec(&json!({ "type": "webauthn.get", "challenge": challenge, "origin": c.base })).unwrap();
    let mut data = Sha256::digest(host.as_bytes()).to_vec();
    data.extend_from_slice(&[0x05, 0, 0, 0, 0]);
    let mut msg = data.clone();
    msg.extend_from_slice(&Sha256::digest(&cd));
    json!({
        "id": id,
        "clientDataJSON": B64.encode(&cd),
        "authenticatorData": B64.encode(&data),
        "signature": B64.encode(k.sign(&msg).to_bytes()),
    })
}

/// A device of `account` of `kind` that `by` certified, approved.
fn device(c: &Control, by: &DeviceKeys, account: &str, kind: Kind, name: &str) -> DeviceKeys {
    let k = DeviceKeys::generate();
    let mut cert = Cert::new(&k, account, kind, name);
    cert.sign_with(by);
    c.app.db.put_device(&cert, true, now_ms()).unwrap();
    k
}

/// Team `TEAM`, founded by a1 (`root`), at `v1`.
fn team(c: &Control, root: &DeviceKeys, v1: &Roster) {
    let t = crate::db::Team {
        id: TEAM.into(),
        name: "Acme".into(),
        founder: "a1".into(),
        founder_root: root.id(),
        locked: false,
    };
    c.app.db.add_team(&t, 1, &serde_json::to_string(v1).unwrap(), now_ms()).unwrap();
}

fn path() -> String {
    format!("/api/teams/{TEAM}/roster")
}

/// The body that sends `r`, with `passkey`'s assertion over its bytes.
fn with_passkey(c: &Control, r: &Roster, pk: Option<(&str, &ed25519_dalek::SigningKey)>) -> Value {
    let mut b = json!({ "roster": r });
    if let Some((id, k)) = pk {
        b["passkey"] = assertion(c, id, k, &r.owner_challenge());
    }
    b
}

/// An owner made from a CLI or the desktop app, with the owner's passkey
/// over that version: taken, and its proof kept beside it for anyone to
/// check against the published log. Without a passkey, refused; with one
/// over another version's bytes, or a non-owner's, refused. Removing an
/// owner takes one too; an editor or a viewer, none.
#[tokio::test]
async fn an_owner_made_from_a_cli_takes_the_owners_passkey() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let bob = person(&c.app, "bob", "b1");
    let cli = device(&c, &root, "a1", Kind::Cli, "Arugula CLI");
    let app = device(&c, &root, "a1", Kind::App, "Arugula app on box");
    let jakes = site_passkey(&c, "a1", "pk-a1", 1);
    let bobs = site_passkey(&c, "b1", "pk-b1", 2);
    let v1 = roster(TEAM, 1, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Editor)], &root);
    team(&c, &root, &v1);
    let (jake, bob_s) = (session(&c.app, "a1"), session(&c.app, "b1"));

    // An editor made a viewer, by the browser: no passkey, as before.
    let v2 = roster(TEAM, 2, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Viewer)], &root);
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v2, None))).await;
    assert_eq!(st, 200, "{v}");

    // The CLI makes Bob an owner.
    let v3 = roster(TEAM, 3, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Owner)], &cli);
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v3, None))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("passkey_required")), "no passkey: {v}");
    // Over another version's bytes: the app's, which Jake didn't confirm.
    let other = roster(TEAM, 3, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Owner)], &app);
    let mut b = json!({ "roster": v3 });
    b["passkey"] = assertion(&c, "pk-a1", &jakes, &other.owner_challenge());
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(b)).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("passkey")), "another version's bytes: {v}");
    // Bob, a viewer, confirming his own promotion with his passkey.
    let (st, v) = c.as_person(&bob_s, "POST", &path(), Some(with_passkey(&c, &v3, Some(("pk-b1", &bobs))))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("not_an_owner")), "a non-owner's passkey: {v}");
    // Bob's passkey sent from Jake's session: not Jake's.
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v3, Some(("pk-b1", &bobs))))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("passkey")), "another account's passkey: {v}");
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v3, Some(("pk-a1", &jakes))))).await;
    assert_eq!(st, 200, "the CLI's owner, with Jake's passkey: {v}");
    let now: Roster = serde_json::from_str(&c.app.db.latest_roster(TEAM).unwrap().unwrap()).unwrap();
    assert_eq!(now.member("b1").map(|m| m.role), Some(TeamRole::Owner), "the platform reads the roster now");

    // Kept beside the version, handed out with the rosters, and anyone
    // checks it against the published passkey log.
    let (st, got) = c.as_person(&bob_s, "GET", &format!("/api/teams/{TEAM}/rosters?since=0"), None).await;
    assert_eq!(st, 200, "{got}");
    let proofs = got["owner_proofs"].as_object().unwrap();
    assert_eq!(proofs.keys().collect::<Vec<_>>(), ["3"], "only the owner change: {got}");
    let proof: OwnerProof = serde_json::from_value(proofs["3"].clone()).unwrap();
    assert_eq!((proof.account.as_str(), proof.passkey.id.as_str()), ("a1", "pk-a1"));
    let (st, log) = c.as_person(&bob_s, "GET", "/api/accounts/a1/passkeys/log", None).await;
    assert_eq!(st, 200, "{log}");
    let log: passkey::Log = serde_json::from_value(log).unwrap();
    let origins = passkey::origins_of_sites([c.base.as_str()]);
    assert!(proof.checks(&v3, &v2, &log, &origins).is_ok());

    // The app removes Bob, an owner: that takes Jake's passkey too.
    let v4 = roster(TEAM, 4, vec![member("a1", &root, TeamRole::Owner)], &app);
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v4, None))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("passkey_required")), "removing an owner: {v}");
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v4, Some(("pk-a1", &jakes))))).await;
    assert_eq!(st, 200, "{v}");
}

/// A machine checking the team from before DK7 refuses an owner a CLI or
/// the app made (and every version after), so control holds one back
/// until every such machine is updated, and says which; from a browser
/// all the way back, it goes through (those machines take that).
#[tokio::test]
async fn an_owner_from_a_cli_waits_for_machines_from_before() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let bob = person(&c.app, "bob", "b1");
    let cli = device(&c, &root, "a1", Kind::Cli, "Arugula CLI");
    let jakes = site_passkey(&c, "a1", "pk-a1", 1);
    let v1 = roster(TEAM, 1, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Editor)], &root);
    team(&c, &root, &v1);
    let jake = session(&c.app, "a1");
    let old = daemon(&c.app, "a1", "oldbox");
    c.app.db.set_daemon_team(&old.id(), TEAM).unwrap();
    c.app.db.set_daemon_features(&old.id(), "presigned-invites,owner-moves,join-secret").unwrap();

    let by = |version, k: &DeviceKeys| {
        roster(TEAM, version, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Owner)], k)
    };
    let v2 = by(2, &cli);
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v2, Some(("pk-a1", &jakes))))).await;
    assert_eq!(st, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("oldbox needs an update"), "{v}");
    let v2 = by(2, &root);
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v2, Some(("pk-a1", &jakes))))).await;
    assert_eq!(st, 200, "from the browser: {v}");

    // Updated: the CLI demotes and promotes again.
    c.app.db.set_daemon_features(&old.id(), "presigned-invites,owner-moves,join-secret,owner-passkeys").unwrap();
    let v3 = roster(TEAM, 3, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Editor)], &root);
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v3, Some(("pk-a1", &jakes))))).await;
    assert_eq!(st, 200, "{v}");
    let v4 = by(4, &cli);
    let (st, v) = c.as_person(&jake, "POST", &path(), Some(with_passkey(&c, &v4, Some(("pk-a1", &jakes))))).await;
    assert_eq!(st, 200, "{v}");
}

/// Rosters from before DK7 count as they are: a team whose owners a
/// browser made, with no proof beside any version, still loads, its
/// members' pages still get every version, and its editors and viewers
/// still change with no passkey.
#[tokio::test]
async fn rosters_from_before_still_load() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let bob = person(&c.app, "bob", "b1");
    let carol = person(&c.app, "carol", "c1");
    let v1 = roster(TEAM, 1, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Editor)], &root);
    team(&c, &root, &v1);
    // Made an owner by the browser before DK7, kept with no proof.
    let v2 = roster(
        TEAM,
        2,
        vec![
            member("a1", &root, TeamRole::Owner),
            member("b1", &bob, TeamRole::Owner),
            member("c1", &carol, TeamRole::Viewer),
        ],
        &root,
    );
    c.app.db.add_roster(TEAM, 2, &serde_json::to_string(&v2).unwrap()).unwrap();
    let (st, got) =
        c.as_person(&session(&c.app, "c1"), "GET", &format!("/api/teams/{TEAM}/rosters?since=0"), None).await;
    assert_eq!(st, 200, "{got}");
    assert_eq!(got["rosters"].as_array().unwrap().len(), 2);
    assert_eq!(got["owner_proofs"], json!({}));
    let rosters: Vec<Roster> = serde_json::from_value(got["rosters"].clone()).unwrap();
    let pin = crate::teams::pin(&c.app.db.team(TEAM).unwrap().unwrap());
    let certs = crate::teams::certs_for_test(&c.app, &["a1".into(), "b1".into(), "c1".into()]);
    assert_eq!(arugula_e2e::checkpoint::advance(None, rosters, &pin, &certs).0.map(|r| r.version), Some(2));
    // Bob, an owner from before, makes Carol an editor: no passkey.
    let v3 = roster(
        TEAM,
        3,
        vec![
            member("a1", &root, TeamRole::Owner),
            member("b1", &bob, TeamRole::Owner),
            member("c1", &carol, TeamRole::Editor),
        ],
        &bob,
    );
    let (st, v) = c.as_person(&session(&c.app, "b1"), "POST", &path(), Some(json!({ "roster": v3 }))).await;
    assert_eq!(st, 200, "{v}");
}
