//! How many hub calls a platform request makes once the cache is warm:
//! none (platform step 2, #1150). Out of process every hub call is a round
//! trip over the private network, between Fly regions in production, so a
//! read that asks the hub again on every request shows up in p95. These
//! tests are the guard: a second identical request asks the hub nothing.
//!
//! They also hold what the cache mustn't break: a machine that just joined
//! isn't refused, a machine that left gets no new grant from a stale answer,
//! and a member who left a team is out once the feed says so. And what it
//! keeps for a quiet control: 30 s on, with the feed read throughout, a
//! request still asks nothing (but billing's meters), while a feed outage
//! or gap drops everything (`cache`'s top says why that's safe).

use std::{collections::BTreeMap, sync::Arc};

use arugula_e2e::team::TeamRole;
use serde_json::Value;

use crate::chat_tests::{TestHub, call, seed, serve};

/// The hub calls one request made, by method, and its answer.
async fn counted(hub: &TestHub, base: &str, who: &str, path: &str) -> (BTreeMap<&'static str, usize>, Value) {
    let before = hub.calls_by_method();
    let (st, v) = call(base, who, "GET", path, None).await;
    assert_eq!(st, 200, "{path}: {v}");
    let after = hub.calls_by_method();
    let made =
        after.into_iter().map(|(m, n)| (m, n - before.get(m).copied().unwrap_or(0))).filter(|(_, n)| *n > 0).collect();
    (made, v)
}

/// A machine's request, as the test gate takes it.
async fn as_machine(base: &str, account: &str, machine: &str, path: &str) -> (u16, Value) {
    let r = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .header("x-test-daemon", format!("{account} {machine}"))
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// The regression guard: the second of two identical requests to each of
/// these routes makes no hub call at all.
#[tokio::test]
async fn a_second_request_asks_the_hub_nothing() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    let mut asked = vec![];
    for (who, path) in [
        ("a", "/api/grants"),
        ("b", "/api/chat"),
        ("a", "/api/workspaces"),
        ("a", "/api/billing"),
        ("b", "/api/chat/search?q=from%3Aa%20hello"),
        ("b", "/api/projects"),
    ] {
        let (first, _) = counted(&hub, &base, who, path).await;
        let (second, _) = counted(&hub, &base, who, path).await;
        eprintln!("{path}: first {first:?}, second {second:?}");
        asked.extend((!second.is_empty()).then(|| format!("{path}: {second:?}")));
    }
    assert_eq!(asked, Vec::<String>::new(), "a second request asked the hub");
    // A machine driving its owner's Cloud workspaces: its first call gives
    // it its grant (on the hub's word), and after that nothing.
    assert_eq!(as_machine(&base, "a", "m0", "/api/workspaces").await.0, 200);
    let calls = hub.calls();
    assert_eq!(as_machine(&base, "a", "m0", "/api/workspaces").await.0, 200);
    assert_eq!(hub.calls(), calls, "a machine with a live grant: no hub call");
    assert_eq!(hub.calls_of("resolve"), 0, "the hub vouched for the caller: nobody asks it again");
}

/// A machine that joined after the cache last heard of it is asked about
/// again, not refused: the cache's "no such machine" is never the last word
/// before a grant.
#[tokio::test]
async fn a_machine_that_just_joined_is_not_refused() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    // The cache has heard there's no m9.
    assert!(app.cache.daemons(&*app.hub, &["m9".into()]).await.unwrap().is_empty());
    hub.daemon("m9", "a", None);
    let (st, v) = as_machine(&base, "a", "m9", "/api/workspaces").await;
    assert_eq!(st, 200, "joined before the feed was read: {v}");
    // And its grant is listed by name, from what the hub said.
    let (_, v) = counted(&hub, &base, "a", "/api/grants").await;
    let names: Vec<&str> =
        v["grants"].as_array().unwrap().iter().filter_map(|g| g["subject"]["name"].as_str()).collect();
    assert!(names.contains(&"m9-box"), "{v}");
}

/// A machine that left loses its grant through the projects feed, and the
/// grant list doesn't give it a new one from the cache's older answer.
#[tokio::test]
async fn a_machine_that_left_gets_no_grant_from_a_stale_answer() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    let live = |v: &Value, id: &str| {
        v["grants"].as_array().unwrap().iter().any(|g| g["subject"]["id"] == id && g["live"] == true)
    };
    let (_, v) = counted(&hub, &base, "a", "/api/grants").await;
    assert!(live(&v, "m0"), "{v}");
    // The projects follower's first read only sets where it starts.
    crate::projects::follow(&app).await.unwrap();
    hub.leave("m0");
    // The projects follower reads the leave (its grant goes); the cache
    // hasn't read it yet and still says m0 is a's.
    crate::projects::follow(&app).await.unwrap();
    let (_, v) = counted(&hub, &base, "a", "/api/grants").await;
    assert!(!live(&v, "m0"), "re-granted from a stale answer: {v}");
    crate::cache::catch_up(&app).await.unwrap();
    let (made, v) = counted(&hub, &base, "a", "/api/grants").await;
    assert!(!live(&v, "m0"), "{v}");
    let (again, _) = counted(&hub, &base, "a", "/api/grants").await;
    assert_eq!(again, BTreeMap::new(), "after {made:?}, kept");
}

/// Billing's team list follows the roster through the feed: a member who
/// left is out once the feed is read, within `cache::DELAY`.
#[tokio::test]
async fn a_member_who_left_is_out_of_billing_once_the_feed_says_so() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    let teams = |v: &Value| v["teams"].as_array().unwrap().len();
    let (_, v) = counted(&hub, &base, "b", "/api/billing").await;
    assert_eq!(teams(&v), 1, "{v}");
    assert_eq!(v["relay"]["bytes"], 1024, "{v}");
    hub.team("t", &[("a", TeamRole::Owner)]);
    crate::cache::catch_up(&app).await.unwrap();
    let (_, v) = counted(&hub, &base, "b", "/api/billing").await;
    assert_eq!(teams(&v), 0, "b left t: {v}");
}

/// The feed read every tick for `secs` seconds of the cache's clock, as
/// the platform's follower does.
async fn read_for(app: &crate::Platform, secs: u64) {
    for _ in 0..secs {
        app.cache.advance(crate::cache::FEED_TICK);
        crate::cache::catch_up(app).await.unwrap();
    }
}

const ROUTES: [(&str, &str); 5] = [
    ("a", "/api/grants"),
    ("b", "/api/chat"),
    ("a", "/api/workspaces"),
    ("a", "/api/billing"),
    ("b", "/api/chat/search?q=from%3Aa%20hello"),
];

/// Most of a quiet control's requests come more than a cache tick apart.
/// While the feed is read, what it covers is kept however old: 30 s on, a
/// repeat request asks the hub nothing but billing's meters, which the feed
/// doesn't cover and so run out at the cache's tick.
#[tokio::test]
async fn thirty_seconds_later_a_request_still_asks_the_hub_nothing() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    for (who, path) in ROUTES {
        counted(&hub, &base, who, path).await;
    }
    read_for(&app, 30).await;
    let mut asked = BTreeMap::new();
    for (who, path) in ROUTES {
        let (made, _) = counted(&hub, &base, who, path).await;
        if !made.is_empty() {
            asked.insert(path, made);
        }
    }
    assert_eq!(asked, BTreeMap::from([("/api/billing", BTreeMap::from([("meters", 1)]))]));
}

/// A member taken out of a team is out once the feed is read, however long
/// the cache had kept the team: the feed carries it, within a feed tick.
#[tokio::test]
async fn a_roster_removal_still_revokes_through_the_feed() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    // Chat stays on for b out of t.
    hub.flag("account:b", true);
    crate::cache::catch_up(&app).await.unwrap();
    let general = |v: &Value| v["rooms"].as_array().unwrap().iter().any(|r| r["id"] == "ch_general");
    let (_, v) = counted(&hub, &base, "b", "/api/chat").await;
    assert!(general(&v), "{v}");
    read_for(&app, 30).await;
    hub.team("t", &[("a", TeamRole::Owner)]);
    let (made, v) = counted(&hub, &base, "b", "/api/chat").await;
    assert!(made.is_empty() && general(&v), "until the feed is read: {made:?}");
    // The follower's next tick.
    read_for(&app, 1).await;
    let (_, v) = counted(&hub, &base, "b", "/api/chat").await;
    assert!(!general(&v), "b left t: {v}");
    let (_, v) = counted(&hub, &base, "b", "/api/billing").await;
    assert_eq!(v["teams"].as_array().unwrap().len(), 0, "{v}");
}

/// A feed that can't be read: what's kept is kept for the grace, then
/// everything goes, so a member taken out during the outage is out within
/// the documented delay.
#[tokio::test]
async fn a_feed_outage_drops_everything() {
    use crate::cache::{DELAY, FEED_TICK, GRACE};
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    // Chat stays on for b out of t.
    hub.flag("account:b", true);
    crate::cache::catch_up(&app).await.unwrap();
    let general = |v: &Value| v["rooms"].as_array().unwrap().iter().any(|r| r["id"] == "ch_general");
    counted(&hub, &base, "b", "/api/chat").await;
    read_for(&app, 30).await;
    hub.feed_down(true);
    hub.team("t", &[("a", TeamRole::Owner)]);
    let mut waited = std::time::Duration::ZERO;
    loop {
        app.cache.advance(FEED_TICK);
        waited += FEED_TICK;
        assert!(crate::cache::catch_up(&app).await.is_err());
        let (made, v) = counted(&hub, &base, "b", "/api/chat").await;
        if !general(&v) {
            assert!(!made.is_empty(), "asked the hub again");
            break;
        }
        assert!(made.is_empty(), "within the grace, kept: {made:?}");
        assert!(waited < GRACE, "still in after {waited:?}");
    }
    assert!(waited <= GRACE && GRACE <= DELAY);
}

/// A gap in the feed (changes pruned before they were read) drops
/// everything.
#[tokio::test]
async fn a_feed_gap_drops_everything() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    counted(&hub, &base, "a", "/api/grants").await;
    let (made, _) = counted(&hub, &base, "a", "/api/grants").await;
    assert!(made.is_empty());
    hub.gap();
    crate::cache::catch_up(&app).await.unwrap();
    let (made, _) = counted(&hub, &base, "a", "/api/grants").await;
    assert!(!made.is_empty(), "asked again after a gap");
}

/// What the feed doesn't cover still runs out on time with the feed read
/// throughout: billing's meters are asked again after a cache tick, and
/// nothing else is.
#[tokio::test]
async fn what_the_feed_cant_say_still_runs_out_on_time() {
    use crate::cache::CACHE_TICK;
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    counted(&hub, &base, "a", "/api/billing").await;
    read_for(&app, CACHE_TICK.as_secs() - 1).await;
    let (made, _) = counted(&hub, &base, "a", "/api/billing").await;
    assert_eq!(made, BTreeMap::new(), "within the cache's tick");
    read_for(&app, 1).await;
    let (made, _) = counted(&hub, &base, "a", "/api/billing").await;
    assert_eq!(made, BTreeMap::from([("meters", 1)]), "at it");
}

/// One `Server-Timing` entry's number: `dur` for a time, `desc` for a
/// count (`crate::timing`).
pub(crate) fn timing_entry(header: &str, name: &str) -> Option<f64> {
    header.split(',').map(str::trim).find_map(|e| {
        let mut parts = e.split(';');
        (parts.next()? == name).then(|| parts.find_map(|p| p.split_once('=').and_then(|(_, v)| v.parse().ok())))?
    })
}

/// A request's `Server-Timing`, as the platform answered it.
async fn timing(base: &str, who: &str, path: &str) -> String {
    let r = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .header("cookie", format!("ilg_session=session-{who}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{path}");
    let h = r.headers().get(crate::timing::HEADER).expect("every platform answer says its timing");
    h.to_str().unwrap().to_owned()
}

/// What a request costs in statements once it's warm (each a round trip on
/// Postgres), read from the platform's own `Server-Timing`, so the header
/// and the guard are one. The seed: a's five machines (all with grants),
/// team t's channel and thirty pane threads, b reading fifteen of them.
/// `/api/chat` is five: the reader's notification defaults and their DM
/// people's pauses (#113, #115) are one more, whatever the page's size.
#[tokio::test]
async fn statements_per_request() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    let mut said = vec![];
    for (who, path, most) in
        [("a", "/api/grants", 1), ("b", "/api/chat", 5), ("a", "/api/workspaces", 3), ("a", "/api/billing", 2)]
    {
        timing(&base, who, path).await;
        let h = timing(&base, who, path).await;
        let n = timing_entry(&h, "dbq").unwrap() as u64;
        assert_eq!(timing_entry(&h, "hubn"), Some(0.0), "{path}: {h}");
        assert!(timing_entry(&h, "app").is_some() && timing_entry(&h, "db").is_some(), "{h}");
        assert!(!h.contains("dbconn") && !h.contains("dbprep"), "warm: {h}");
        eprintln!("{path}: {h}");
        said.extend((n > most).then(|| format!("{path}: {n} statements (at most {most})")));
    }
    assert_eq!(said, Vec::<String>::new());
}

/// A call after a write in the hub reads the feed first (`cache::fresh`),
/// but a slow hub holds it back at most `cache::FRESH_WAIT`, not the
/// internal client's 30 s: it's answered from what's kept, and its
/// `Server-Timing` shows the hub call it waited on.
#[tokio::test]
async fn a_slow_feed_holds_a_call_back_briefly() {
    use std::{sync::atomic::Ordering, time::Instant};
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    crate::cache::catch_up(&app).await.unwrap();
    timing(&base, "a", "/api/grants").await;
    hub.feed_slow.store(true, Ordering::SeqCst);
    let ahead = u64::MAX / 2;
    let t = Instant::now();
    let r = reqwest::Client::new()
        .get(format!("{base}/api/grants"))
        .header("cookie", "ilg_session=session-a")
        .header(crate::cache::SEQ_HEADER, ahead.to_string())
        .send()
        .await
        .unwrap();
    let took = t.elapsed();
    assert_eq!(r.status(), 200);
    assert!(took < crate::cache::FRESH_WAIT * 4, "waited {took:?} on a slow feed");
    let h = r.headers()[crate::timing::HEADER].to_str().unwrap().to_owned();
    assert_eq!(timing_entry(&h, "hubn"), Some(1.0), "{h}");
    assert!(timing_entry(&h, "hub").unwrap() >= crate::cache::FRESH_WAIT.as_secs_f64() * 1e3 * 0.9, "{h}");
}

/// The platform page's projects (M150): the reader's own first, then each
/// team's default one with its members, read only.
#[tokio::test]
async fn the_projects_are_mine_then_my_teams() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    hub.team("u", &[("b", TeamRole::Owner), ("c", TeamRole::Viewer)]);
    let (st, v) = call(&base, "b", "GET", "/api/projects", None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["me"]["account"], "b");
    let p = v["projects"].as_array().unwrap();
    assert_eq!(p.len(), 3, "{v}");
    assert_eq!((p[0]["name"].as_str(), &p[0]["team"]), (Some("Personal"), &Value::Null));
    assert!(p[0]["id"].as_str().unwrap().starts_with("prj_"));
    assert_eq!(p[1]["team"]["id"], "t");
    assert_eq!(p[1]["team"]["role"], "editor");
    assert_eq!(p[1]["team"]["members"][0], serde_json::json!({ "account": "a", "name": "a", "role": "owner" }));
    assert_eq!((p[2]["team"]["id"].as_str(), p[2]["team"]["role"].as_str()), (Some("u"), Some("owner")));
    // The same project every time, and the one chat's channels belong to.
    let (_, again) = call(&base, "b", "GET", "/api/projects", None).await;
    assert_eq!(again, v);
    assert_eq!(app.db.team_default_project("u").unwrap().as_deref(), p[2]["id"].as_str());

    // Someone in one team of the two: their own, and that team's.
    let (_, c) = call(&base, "c", "GET", "/api/projects", None).await;
    let names: Vec<_> =
        c["projects"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["Personal", "u"], "{c}");
    assert_eq!(c["projects"][1]["team"]["role"], "viewer");
}
