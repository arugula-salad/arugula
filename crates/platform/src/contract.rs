//! The contract every [`HubPort`] keeps. Control runs [`run`] against its
//! in-process port (`hub_port::tests`), and M140 runs it against the HTTP
//! port, so the platform behaves the same either way.
//!
//! A [`Fixture`] seeds the hub as the hub itself would (accounts, devices,
//! teams, machines, shares, flags, meters, push subscriptions) and hands out
//! the port to check. The checks use ids of their own, so one fixture runs
//! them all, once.

use std::{collections::BTreeMap, sync::Arc};

use arugula_control_common::Principal;
use arugula_e2e::team::TeamRole;

use crate::hub::{Account, Change, ChangeKind, Daemon, HubPort, Meter, PushOptions, Pushed};

/// The hub behind a port, seeded the way the hub writes it.
pub trait Fixture: Send + Sync {
    /// The port under test.
    fn hub(&self) -> Arc<dyn HubPort>;
    /// An account signed in with GitHub as `login`, with a display name
    /// (`""` for none).
    fn account(&self, id: &str, login: &str, name: &str) -> anyhow::Result<()>;
    /// Its display name.
    fn rename(&self, account: &str, name: &str) -> anyhow::Result<()>;
    /// An approved CLI device of the account: its id.
    fn cli(&self, account: &str) -> anyhow::Result<String>;
    /// The account revokes one of its devices.
    fn revoke(&self, account: &str, device: &str) -> anyhow::Result<()>;
    /// A team's next roster, signed by `founder`'s device: the first one
    /// makes the team. Members are (account, role), named after their
    /// account.
    fn roster(&self, team: &str, name: &str, founder: &str, members: &[(&str, TeamRole)]) -> anyhow::Result<()>;
    fn lock(&self, team: &str, locked: bool) -> anyhow::Result<()>;
    /// A machine joined to an account, or the name of one that did.
    fn daemon(&self, id: &str, account: &str, name: &str) -> anyhow::Result<()>;
    /// A machine moves to a team, or out of one.
    fn move_daemon(&self, id: &str, team: Option<&str>) -> anyhow::Result<()>;
    /// A machine lets an account in, and the account says yes.
    fn share(&self, daemon: &str, account: &str) -> anyhow::Result<()>;
    /// A machine leaves its account.
    fn leave(&self, daemon: &str) -> anyhow::Result<()>;
    fn flag(&self, owner: &str, name: &str, on: Option<bool>) -> anyhow::Result<()>;
    /// Bytes the relay carried for an account on the day `ms` falls in.
    fn relay_bytes(&self, account: &str, ms: u64, bytes: u64) -> anyhow::Result<()>;
    /// The account deletes itself.
    fn delete(&self, account: &str) -> anyhow::Result<()>;
    /// A push subscription of one of the account's approved devices, at
    /// the fixture's own push service, which takes what it's sent, or with
    /// `gone` answers 410 as for a subscription that's gone.
    fn subscribe(&self, account: &str, gone: bool) -> anyhow::Result<()>;
    /// How many pushes the fixture's push service took.
    fn pushes(&self) -> usize;
    /// A push subscription of one of the account's devices, at the
    /// fixture's push service, and then the account revokes that device.
    fn subscribe_revoked(&self, account: &str) -> anyhow::Result<()>;
    /// The `TTL` the last push the fixture's push service took asked for.
    fn last_ttl(&self) -> Option<u32>;
    /// The hub's prune of its change feed, as it would run at `now` (ms).
    fn prune_changes(&self, now: u64) -> anyhow::Result<()>;
    /// A machine that joins the account the way a daemon does, approved,
    /// which gives it `cloud.drive` over the account (the platform's, from
    /// the feed or its first ask): its id.
    fn machine(&self, account: &str, name: &str) -> anyhow::Result<String>;
    /// The account revokes its machine's `cloud.drive` grant (the
    /// platform's, as `POST /api/grants/{id}/revoke` does).
    fn revoke_cloud_drive(&self, account: &str, daemon: &str) -> anyhow::Result<()>;
}

/// Every check, in turn.
pub async fn run(f: &dyn Fixture) {
    principals(f).await;
    names(f).await;
    teams(f).await;
    daemons(f).await;
    reach(f).await;
    flags(f).await;
    meters(f).await;
    push(f).await;
    push_options(f).await;
    cloud(f).await;
    projects(f).await;
    pokes(f).await;
    // Last: it prunes the feed.
    changes(f).await;
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn account(id: &str, login: &str, name: &str) -> Account {
    Account { id: id.into(), login: login.into(), name: name.into() }
}

async fn principals(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-pa", "pa-login", "Pa").unwrap();
    f.account("acct-pb", "pb-login", "").unwrap();
    let cli = f.cli("acct-pa").unwrap();
    f.daemon("dmn-pa", "acct-pa", "pa-box").unwrap();
    let resolve = |p: Principal| {
        let hub = hub.clone();
        async move { hub.resolve(&p).await.unwrap() }
    };

    let me = resolve(Principal::Person { account: "acct-pa".into() }).await.expect("a person with an account");
    assert_eq!(me.account, account("acct-pa", "pa-login", "Pa"));
    assert_eq!(me.daemon, None);
    assert!(resolve(Principal::Person { account: "acct-none".into() }).await.is_none(), "no such account");

    let c = Principal::Cli { account: "acct-pa".into(), device: cli.clone() };
    assert_eq!(resolve(c.clone()).await.expect("its CLI device").principal, c);
    let theirs = Principal::Cli { account: "acct-pb".into(), device: cli.clone() };
    assert!(resolve(theirs).await.is_none(), "another account's device");
    assert!(resolve(Principal::Cli { account: "acct-pa".into(), device: "cli-none".into() }).await.is_none());

    let d = Principal::Daemon { account: "acct-pa".into(), daemon: "dmn-pa".into() };
    let box_ = Daemon { id: "dmn-pa".into(), account: "acct-pa".into(), team: None, name: "pa-box".into() };
    assert_eq!(resolve(d.clone()).await.expect("its machine").daemon, Some(box_));
    let theirs = Principal::Daemon { account: "acct-pb".into(), daemon: "dmn-pa".into() };
    assert!(resolve(theirs).await.is_none(), "another account's machine");

    // M218: an account's devices as the hub keeps them, its first approved
    // one its root; nothing for an account that isn't.
    let t = hub.device_trust("acct-pa").await.unwrap().expect("an account with a device");
    assert_eq!(t.root, cli);
    assert!(t.certs.iter().any(|c| c.device == cli) && t.revocations.is_empty());
    assert_eq!(hub.device_trust("acct-none").await.unwrap(), None);
    // M218: a passkey assertion the hub doesn't know is refused, saying why.
    let unknown = crate::hub::PasskeyAssertion {
        id: "pk-none".into(),
        client_data_json: String::new(),
        authenticator_data: String::new(),
        signature: String::new(),
    };
    let v = hub.verify_passkey("acct-pa", "challenge", &unknown).await.unwrap();
    assert!(!v.ok && !v.why.is_empty(), "{v:?}");

    f.revoke("acct-pa", &cli).unwrap();
    assert!(resolve(c).await.is_none(), "a revoked device");
    f.leave("dmn-pa").unwrap();
    assert!(resolve(d).await.is_none(), "a machine that left");
}

async fn names(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-na", "na-login", "").unwrap();
    f.account("acct-nb", "nb-login", "Nora B").unwrap();
    f.account("acct-nc", "nc-login", "nora b").unwrap();
    f.account("acct-nd", "nd-login", "Nora B").unwrap();

    let mut got = hub.accounts(&ids(&["acct-nb", "acct-none", "acct-na"])).await.unwrap();
    got.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(
        got,
        [account("acct-na", "na-login", "na-login"), account("acct-nb", "nb-login", "Nora B")],
        "with no name of its own, an account goes by its GitHub login; one that isn't there is left out"
    );
    assert!(hub.accounts(&[]).await.unwrap().is_empty());

    let called = |w: &'static str| {
        let hub = hub.clone();
        async move {
            let mut v: Vec<String> = hub.accounts_called(w).await.unwrap().into_iter().map(|a| a.id).collect();
            v.sort();
            v
        }
    };
    assert_eq!(called("NA-LOGIN").await, ["acct-na"], "a login, in any case");
    assert_eq!(called("Nora B").await.len(), 2, "three chose that name: at most two, to tell one from many");
    assert!(called("nobody").await.is_empty());
    assert!(called("").await.is_empty());

    f.rename("acct-nb", "Nora").unwrap();
    assert_eq!(hub.accounts(&ids(&["acct-nb"])).await.unwrap()[0].name, "Nora");
}

async fn teams(f: &dyn Fixture) {
    let hub = f.hub();
    for a in ["acct-ta", "acct-tb", "acct-tc"] {
        f.account(a, a, "").unwrap();
    }
    use TeamRole::{Editor, Owner, Viewer};
    f.roster(
        "t-one",
        "One",
        "acct-ta",
        &[("acct-ta", Owner), ("acct-tb", Editor), ("acct-tc", Viewer), ("acct-gone", Viewer)],
    )
    .unwrap();
    assert_eq!(hub.teams_of("acct-tb").await.unwrap(), ["t-one"]);
    assert!(hub.teams_of("acct-none").await.unwrap().is_empty());

    let t = hub.teams(&ids(&["t-none", "t-one"])).await.unwrap();
    assert_eq!(t.len(), 1, "a team that isn't there is left out");
    let t = &t[0];
    assert_eq!((t.id.as_str(), t.name.as_str(), t.founder.as_str()), ("t-one", "One", "acct-ta"));
    assert_eq!((t.version, t.locked), (1, false));
    let mut members: Vec<(&str, TeamRole)> = t.members.iter().map(|m| (m.account.as_str(), m.role)).collect();
    members.sort_by_key(|(a, _)| *a);
    assert_eq!(members, [("acct-ta", Owner), ("acct-tb", Editor), ("acct-tc", Viewer)], "only members with an account");
    assert_eq!(t.members.iter().find(|m| m.account == "acct-tb").unwrap().name, "acct-tb");
    // M218: the rosters as signed, oldest first; none for a team that isn't.
    let rosters = hub.team_rosters("t-one").await.unwrap();
    assert_eq!(rosters.iter().map(|r| r.version).collect::<Vec<_>>(), [1]);
    assert!(rosters[0].member("acct-gone").is_some(), "as signed, not as filtered");
    assert!(hub.team_rosters("t-none").await.unwrap().is_empty());

    f.roster("t-one", "Uno", "acct-ta", &[("acct-ta", Owner), ("acct-tb", Editor)]).unwrap();
    let t = &hub.teams(&ids(&["t-one"])).await.unwrap()[0];
    assert_eq!((t.version, t.name.as_str(), t.members.len()), (2, "Uno", 2), "the latest roster");
    assert!(hub.teams_of("acct-tc").await.unwrap().is_empty(), "out of the latest roster, out of the team");

    f.lock("t-one", true).unwrap();
    let t = &hub.teams(&ids(&["t-one"])).await.unwrap()[0];
    assert!(t.locked);
    assert_eq!((t.role("acct-ta"), t.role("acct-tb")), (Some(Owner), None), "a locked team has only its owners");
    f.lock("t-one", false).unwrap();
}

async fn daemons(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-ma", "ma", "").unwrap();
    f.roster("t-m", "M", "acct-ma", &[("acct-ma", TeamRole::Owner)]).unwrap();
    f.daemon("dmn-m1", "acct-ma", "m-one").unwrap();
    f.daemon("dmn-m2", "acct-ma", "m-two").unwrap();
    f.move_daemon("dmn-m2", Some("t-m")).unwrap();
    f.daemon("dmn-m1", "acct-ma", "m-uno").unwrap();

    let mut got = hub.daemons(&ids(&["dmn-m2", "dmn-none", "dmn-m1"])).await.unwrap();
    got.sort_by(|a, b| a.id.cmp(&b.id));
    let d = |id: &str, team: Option<&str>, name: &str| Daemon {
        id: id.into(),
        account: "acct-ma".into(),
        team: team.map(Into::into),
        name: name.into(),
    };
    assert_eq!(got, [d("dmn-m1", None, "m-uno"), d("dmn-m2", Some("t-m"), "m-two")]);

    f.move_daemon("dmn-m2", None).unwrap();
    assert_eq!(hub.daemons(&ids(&["dmn-m2"])).await.unwrap()[0].team, None, "moved out of its team");
    f.leave("dmn-m1").unwrap();
    assert!(hub.daemons(&ids(&["dmn-m1"])).await.unwrap().is_empty(), "a machine that left");
}

async fn reach(f: &dyn Fixture) {
    let hub = f.hub();
    for a in ["acct-ra", "acct-rb", "acct-rd", "acct-re", "acct-rx"] {
        f.account(a, a, "").unwrap();
    }
    f.roster("t-r", "R", "acct-ra", &[("acct-ra", TeamRole::Owner), ("acct-rb", TeamRole::Editor)]).unwrap();
    f.daemon("dmn-r1", "acct-ra", "personal").unwrap();
    f.daemon("dmn-r2", "acct-ra", "team box").unwrap();
    f.move_daemon("dmn-r2", Some("t-r")).unwrap();
    f.daemon("dmn-rs", "acct-rd", "shared").unwrap();
    f.share("dmn-rs", "acct-re").unwrap();

    let all = ids(&["dmn-r1", "dmn-r2", "dmn-rs"]);
    let may = |who: &'static str, of: Vec<String>| {
        let hub = hub.clone();
        async move { hub.may_reach(who, &of).await.unwrap() }
    };
    assert_eq!(
        may("acct-ra", ids(&["dmn-r2", "dmn-rs", "dmn-r1"])).await,
        ["dmn-r2", "dmn-r1"],
        "its own, in the order asked"
    );
    assert_eq!(may("acct-rb", all.clone()).await, ["dmn-r2"], "its team's");
    assert_eq!(may("acct-re", all.clone()).await, ["dmn-rs"], "one shared with it");
    assert_eq!(may("acct-rd", all.clone()).await, ["dmn-rs"]);
    assert!(may("acct-rx", all.clone()).await.is_empty(), "a stranger");
    assert!(may("acct-ra", vec![]).await.is_empty());

    let mut ra = hub.machines_for("acct-ra").await.unwrap();
    ra.retain(|m| all.contains(m));
    assert_eq!(ra, ["dmn-r1", "dmn-r2"]);
    assert_eq!(hub.machines_for("acct-rb").await.unwrap(), ["dmn-r2"]);
    assert_eq!(hub.machines_for("acct-re").await.unwrap(), ["dmn-rs"]);
    assert!(hub.machines_for("acct-rx").await.unwrap().is_empty());

    f.lock("t-r", true).unwrap();
    assert!(may("acct-rb", all.clone()).await.is_empty(), "a locked team's machines are its owners' only");
    assert_eq!(may("acct-ra", all.clone()).await, ["dmn-r1", "dmn-r2"]);
    assert_eq!(hub.machines_for("acct-rb").await.unwrap(), ["dmn-r2"], "still listed: may_reach is the check");
}

async fn flags(f: &dyn Fixture) {
    let hub = f.hub();
    f.flag("account:acct-fa", "chat", Some(true)).unwrap();
    f.flag("account:acct-fa", "beta", Some(false)).unwrap();
    f.flag("team:t-f", "chat", Some(false)).unwrap();
    let got = hub.labs_flags(&ids(&["account:acct-fa", "team:t-f", "account:acct-none"])).await.unwrap();
    let want: BTreeMap<String, BTreeMap<String, bool>> = [
        ("account:acct-fa".to_owned(), [("beta".to_owned(), false), ("chat".to_owned(), true)].into()),
        ("team:t-f".to_owned(), [("chat".to_owned(), false)].into()),
    ]
    .into();
    assert_eq!(got, want, "an owner that set nothing is left out");
    f.flag("account:acct-fa", "beta", None).unwrap();
    let got = hub.labs_flags(&ids(&["account:acct-fa"])).await.unwrap();
    assert_eq!(got["account:acct-fa"], BTreeMap::from([("chat".to_owned(), true)]), "unset");
}

async fn meters(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-mt", "mt", "").unwrap();
    // 2026-09-21 14:13:20 UTC, and the month before.
    let until = 1_790_000_000_000;
    let month_start = 1_788_220_800_000;
    let min = 60_000;
    f.relay_bytes("acct-mt", until - 3_600_000, 1_000).unwrap();
    f.relay_bytes("acct-mt", month_start + 1, 500).unwrap();
    f.relay_bytes("acct-mt", month_start - 1, 9_999).unwrap();
    let got = hub.meters(&ids(&["acct-mt", "acct-none"]), until - 10 * min, until).await.unwrap();
    assert_eq!(
        got,
        [
            Meter { account: "acct-mt".into(), relay_bytes: 1_500, sandbox_minutes: 0 },
            Meter { account: "acct-none".into(), relay_bytes: 0, sandbox_minutes: 0 },
        ],
        "this month's relay bytes; no sandbox minutes since hosted sandboxes went (M190); one per account asked"
    );
    assert!(hub.metered_accounts().await.unwrap().is_empty());
}

async fn push(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-pu", "pu", "").unwrap();
    f.account("acct-pv", "pv", "").unwrap();
    let msg = serde_json::json!({ "title": "Someone mentioned you", "body": "hi", "tag": "chat-r1" });
    assert_eq!(hub.push("acct-pu", &msg).await.unwrap(), Pushed::default(), "nobody subscribed");

    f.subscribe("acct-pu", false).unwrap();
    let before = f.pushes();
    assert_eq!(hub.push("acct-pu", &msg).await.unwrap(), Pushed { sent: 1, gone: 0, failed: 0 });
    assert_eq!(f.pushes(), before + 1, "the push service has it");
    assert_eq!(hub.push("acct-pv", &msg).await.unwrap(), Pushed::default(), "another account's devices hear nothing");

    f.subscribe("acct-pv", true).unwrap();
    assert_eq!(hub.push("acct-pv", &msg).await.unwrap(), Pushed { sent: 0, gone: 1, failed: 0 });
    assert_eq!(hub.push("acct-pv", &msg).await.unwrap(), Pushed::default(), "a subscription that's gone is forgotten");
}

/// `push_with` (M138): how long a push service keeps it, and whether it
/// goes to every subscription (chat's) or only those of approved devices
/// that aren't revoked (`push`'s, and Cloud's).
async fn push_options(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-po", "po", "").unwrap();
    f.subscribe_revoked("acct-po").unwrap();
    let msg = serde_json::json!({ "title": "jake mentioned you in #general", "body": "hi", "tag": "chat-r1" });
    assert_eq!(hub.push("acct-po", &msg).await.unwrap(), Pushed::default(), "a revoked device hears nothing");
    let none = hub.push_with("acct-po", &msg, PushOptions::default()).await.unwrap();
    assert_eq!(none, Pushed::default(), "the default is push's");

    let before = f.pushes();
    let every = PushOptions { ttl: None, any_subscription: true };
    assert_eq!(hub.push_with("acct-po", &msg, every).await.unwrap(), Pushed { sent: 1, gone: 0, failed: 0 });
    assert_eq!((f.pushes(), f.last_ttl()), (before + 1, Some(3600)), "every subscription, with the hub's hour");
    let five = std::time::Duration::from_secs(300);
    let short = PushOptions { ttl: Some(five), any_subscription: true };
    assert_eq!(hub.push_with("acct-po", &msg, short).await.unwrap().sent, 1);
    assert_eq!(f.last_ttl(), Some(300), "the TTL asked for");

    f.subscribe("acct-po", false).unwrap();
    let approved = PushOptions { ttl: Some(five), any_subscription: false };
    assert_eq!(hub.push_with("acct-po", &msg, approved).await.unwrap(), Pushed { sent: 1, gone: 0, failed: 0 });
    assert_eq!(f.last_ttl(), Some(300), "Cloud's approvals: five minutes, approved devices only");
    assert_eq!(hub.push("acct-po", &msg).await.unwrap().sent, 1);
    assert_eq!(f.last_ttl(), Some(3600));
}

/// What chat asked of projects (M138): a team's default project. The
/// platform keeps projects itself since step 2 and no longer asks; the
/// method stays (the trait only grows), and the in-process hub answers it
/// from the platform's own. An HTTP hub that doesn't serve it (M140) can
/// leave this check out.
async fn projects(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-pj", "pj", "").unwrap();
    f.roster("t-pj", "Pj", "acct-pj", &[("acct-pj", TeamRole::Owner)]).unwrap();
    let project = hub.team_project("t-pj").await.unwrap().expect("a team has one");
    assert!(project.starts_with("prj_"), "{project}");
    assert_eq!(hub.team_project("t-pj").await.unwrap(), Some(project.clone()), "the same one after");
    assert_ne!(hub.personal_project("acct-pj").await.unwrap(), Some(project), "not its founder's own");
    assert_eq!(hub.team_project("t-none").await.unwrap(), None, "no such team");
}

/// What Cloud asked (M137): an account's personal project, and a machine's
/// `cloud.drive` grant. Unused by the platform since step 2, as
/// [`projects`]: the in-process hub answers from the platform's own.
async fn cloud(f: &dyn Fixture) {
    let hub = f.hub();
    f.account("acct-cl", "cl", "").unwrap();
    f.account("acct-cm", "cm", "").unwrap();
    let project = hub.personal_project("acct-cl").await.unwrap().expect("an account has one");
    assert!(project.starts_with("prj_"), "{project}");
    assert_eq!(hub.personal_project("acct-cl").await.unwrap(), Some(project.clone()), "the same one after");
    let theirs = hub.personal_project("acct-cm").await.unwrap().expect("each account has its own");
    assert_ne!(theirs, project);
    assert_eq!(hub.personal_project("acct-none").await.unwrap(), None, "no such account");

    let m = f.machine("acct-cl", "cl-box").unwrap();
    assert!(hub.can_drive_cloud(&m, "acct-cl").await.unwrap(), "a machine that joined drives its account's");
    assert!(!hub.can_drive_cloud(&m, "acct-cm").await.unwrap(), "and no other account's");
    assert!(!hub.can_drive_cloud("dmn-none", "acct-cl").await.unwrap(), "no such machine");
    f.revoke_cloud_drive("acct-cl", &m).unwrap();
    assert!(!hub.can_drive_cloud(&m, "acct-cl").await.unwrap(), "a revoked grant");
}

/// Relaying a webhook's pokes (M161): to no daemon when none watches the
/// repository, nothing for no pokes. Who hears one is `forge_wire`'s, over
/// real relay sockets.
async fn pokes(f: &dyn Fixture) {
    let hub = f.hub();
    let poke = arugula_control_wire::forge::Poke {
        provider: "github".into(),
        host: "github.com".into(),
        repo: "nobody/watches".into(),
        number: Some(1),
        event: "pull_request".into(),
        delivery: "d-contract".into(),
    };
    assert_eq!(hub.forge_pokes(&[poke]).await.unwrap(), 0);
    assert_eq!(hub.forge_pokes(&[]).await.unwrap(), 0);
    // M163: who someone is on GitHub; nobody, for an account that isn't.
    assert_eq!(hub.github_user("acct-none").await.unwrap(), None);
    // M235: who signs in with a numeric GitHub id, by the number only (the
    // fixture's identities' subjects are their accounts' ids).
    f.account("7350235", "acct-gh-login", "").unwrap();
    assert_eq!(hub.github_account(7_350_235).await.unwrap().as_deref(), Some("7350235"));
    assert_eq!(hub.github_account(7_350_236).await.unwrap(), None);
    // M180: a machine that isn't on the relay says nothing about whether
    // it's working. What a connected one says is `relay`'s, over a socket.
    assert!(hub.activity(&ids(&["d-not-connected"])).await.unwrap().is_empty());
    assert!(hub.activity(&[]).await.unwrap().is_empty());
    assert!(hub.machines_online(&ids(&["d-not-connected"])).await.unwrap().is_empty());
}

/// Every change after `after`, a page of `limit` at a time.
async fn all_after(hub: &dyn HubPort, mut after: u64, limit: u32) -> Vec<Change> {
    let mut out = Vec::new();
    loop {
        let page = hub.changes(after, limit).await.unwrap();
        assert!(page.changes.len() <= limit as usize, "at most `limit`");
        let Some(last) = page.changes.last() else { return out };
        after = last.seq;
        out.extend(page.changes);
    }
}

async fn changes(f: &dyn Fixture) {
    let hub = f.hub();
    use ChangeKind::*;
    let start = hub.changes(0, 0).await.unwrap();
    assert!(start.changes.is_empty(), "a page of none");
    let start = start.last;
    f.account("acct-fx", "fx", "").unwrap();
    f.account("acct-fy", "fy", "").unwrap();
    f.rename("acct-fx", "Fx").unwrap();
    f.rename("acct-fx", "Fx").unwrap();
    f.roster("t-fx", "Fx", "acct-fx", &[("acct-fx", TeamRole::Owner)]).unwrap();
    f.lock("t-fx", true).unwrap();
    f.daemon("dmn-fx", "acct-fx", "fx-box").unwrap();
    f.move_daemon("dmn-fx", Some("t-fx")).unwrap();
    f.share("dmn-fx", "acct-fy").unwrap();
    f.flag("account:acct-fx", "chat", Some(true)).unwrap();
    f.flag("account:acct-fx", "chat", Some(true)).unwrap();
    let cli = f.cli("acct-fx").unwrap();
    f.revoke("acct-fx", &cli).unwrap();
    f.leave("dmn-fx").unwrap();

    let got = all_after(&*hub, start, 1_000).await;
    let mut seen: Vec<(ChangeKind, &str)> = got.iter().map(|c| (c.kind, c.subject.as_str())).collect();
    seen.dedup();
    assert_eq!(
        seen,
        [
            (AccountName, "acct-fx"),
            (AccountName, "acct-fy"),
            (AccountName, "acct-fx"),
            (TeamRoster, "t-fx"),
            (TeamLocked, "t-fx"),
            (DaemonJoined, "dmn-fx"),
            (DaemonMoved, "dmn-fx"),
            (DaemonAccess, "dmn-fx"),
            (LabsFlag, "account:acct-fx"),
            (DeviceRevoked, cli.as_str()),
            (DaemonLeft, "dmn-fx"),
        ],
        "each change once, in order (making an account is one: its name is new); setting what's already set isn't"
    );
    assert!(got.windows(2).all(|w| w[0].seq < w[1].seq), "seq increases");
    assert!(got.iter().all(|c| c.seq > start && c.at > 0));
    assert_eq!(all_after(&*hub, start, 2).await, got, "pages of two say the same");
    let last = hub.changes(start, 1).await.unwrap().last;
    assert_eq!(last, got.last().unwrap().seq, "`last` is the newest");
    assert!(!hub.changes(start, 1).await.unwrap().gap);

    // Deleting an account leaves one change that names nobody, and takes
    // the earlier ones naming it, its devices, machines and teams with it.
    f.account("acct-fz", "fz", "").unwrap();
    f.rename("acct-fz", "Fz").unwrap();
    f.roster("t-fz", "Fz", "acct-fz", &[("acct-fz", TeamRole::Owner)]).unwrap();
    f.daemon("dmn-fz", "acct-fz", "fz-box").unwrap();
    f.flag("account:acct-fz", "chat", Some(true)).unwrap();
    let before = hub.changes(0, 0).await.unwrap().last;
    f.delete("acct-fz").unwrap();
    let after = all_after(&*hub, before, 1_000).await;
    assert!(after.iter().any(|c| c.kind == AccountDeleted && c.subject.is_empty()), "{after:?}");
    let everything = all_after(&*hub, 0, 1_000).await;
    for gone in ["acct-fz", "dmn-fz", "t-fz"] {
        assert!(!everything.iter().any(|c| c.subject.contains(gone)), "{gone} is still in the feed");
    }

    // Pruned before anyone read it: a gap, and `last` stays.
    let newest = hub.changes(0, 0).await.unwrap().last;
    f.prune_changes(u64::MAX / 2).unwrap();
    let page = hub.changes(start, 100).await.unwrap();
    assert!(page.changes.is_empty() && page.gap, "{page:?}");
    assert_eq!(page.last, newest, "seq is never reused");
    assert!(!hub.changes(newest, 100).await.unwrap().gap, "nothing missed from the newest");
    f.rename("acct-fx", "Fx again").unwrap();
    let page = hub.changes(newest, 100).await.unwrap();
    assert_eq!(page.changes.len(), 1);
    assert!(page.changes[0].seq > newest, "after a prune, seq goes on");
}
