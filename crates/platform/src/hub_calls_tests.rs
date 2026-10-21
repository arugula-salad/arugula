//! How many hub calls a platform request makes once the cache is warm:
//! none (platform step 2, #1150). Out of process every hub call is a round
//! trip over the private network, between Fly regions in production, so a
//! read that asks the hub again on every request shows up in p95. These
//! tests are the guard: a second identical request asks the hub nothing.
//!
//! They also hold what the cache mustn't break: a machine that just joined
//! isn't refused, a machine that left gets no new grant from a stale answer,
//! and a member who left a team is out once the feed says so.

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
        ("a", "/api/cloud/workspaces"),
        ("a", "/api/billing"),
        ("b", "/api/chat/search?q=from%3Aa%20hello"),
    ] {
        let (first, _) = counted(&hub, &base, who, path).await;
        let (second, _) = counted(&hub, &base, who, path).await;
        eprintln!("{path}: first {first:?}, second {second:?}");
        asked.extend((!second.is_empty()).then(|| format!("{path}: {second:?}")));
    }
    assert_eq!(asked, Vec::<String>::new(), "a second request asked the hub");
    // A machine driving its owner's Cloud workspaces: its first call gives
    // it its grant (on the hub's word), and after that nothing.
    assert_eq!(as_machine(&base, "a", "m0", "/api/cloud/workspaces").await.0, 200);
    let calls = hub.calls();
    assert_eq!(as_machine(&base, "a", "m0", "/api/cloud/workspaces").await.0, 200);
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
    let (st, v) = as_machine(&base, "a", "m9", "/api/cloud/workspaces").await;
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
