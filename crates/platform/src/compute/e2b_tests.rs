//! The E2B adapter against a fake of E2B's API: an axum stub that keeps
//! boxes in memory, records every call, and fails on cue.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, Query, State as AxState},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get, post, put},
};
use serde_json::{Value, json};

use super::*;

const KEY: &str = "e2b_test_key";

#[derive(Default)]
struct Fake {
    boxes: BTreeMap<String, (String, Value)>,
    /// Every call: method, path and query, body.
    calls: Vec<(String, String, Value)>,
    /// Answers to give before anything else, in order.
    failures: VecDeque<Response>,
    made: u32,
    /// Boxes per listing page.
    page: usize,
    /// Each box's lifecycle events, oldest first (M188's runs).
    events: BTreeMap<String, Vec<Value>>,
    /// Each box's egress rules, as last set (M185).
    rules: BTreeMap<String, Value>,
    /// Snapshots (M263): each id and the box it was taken of, and how many
    /// were made. `no_snapshots`: as loam's E2B API, every snapshot route
    /// is a 404.
    snapshots: BTreeMap<String, String>,
    snapped: u32,
    no_snapshots: bool,
    /// What each box was made from. As E2B (seen live): a snapshot isn't
    /// deleted (400) while a box made from it exists, a paused one too, nor
    /// for `linger` more tries after its last box is deleted.
    from: BTreeMap<String, String>,
    linger: u32,
    lingering: BTreeMap<String, u32>,
}

type S = Arc<Mutex<Fake>>;

fn record(s: &S, method: &Method, uri: &Uri, body: Value) -> Option<Response> {
    let mut f = s.lock().unwrap();
    f.calls.push((method.to_string(), uri.to_string(), body));
    f.failures.pop_front()
}

fn authed(h: &HeaderMap) -> bool {
    h.get("x-api-key").is_some_and(|k| k == KEY)
}

fn error(status: u16, code: Option<&str>) -> Response {
    let mut body = json!({"code": status, "message": "echoed: ARUGULA_HOSTED_TICKET=secret-ticket"});
    if let Some(c) = code {
        body["error_code"] = json!(c);
    }
    (StatusCode::from_u16(status).unwrap(), Json(body)).into_response()
}

fn listed(id: &str, state: &str, md: &Value) -> Value {
    json!({
        "sandboxID": id, "templateID": "base", "alias": "base", "clientID": "x", "state": state,
        "metadata": md, "startedAt": "2026-10-21T00:00:00Z", "endAt": "2026-10-21T00:05:00Z",
        "cpuCount": 2, "memoryMB": 512, "diskSizeMB": 1024, "envdVersion": "0.6.10",
    })
}

async fn create(AxState(s): AxState<S>, m: Method, uri: Uri, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Some(r) = record(&s, &m, &uri, body.clone()) {
        return r;
    }
    if !authed(&h) {
        return error(401, None);
    }
    let mut f = s.lock().unwrap();
    let from = body["templateID"].as_str().unwrap_or_default();
    if from != "base" && !f.snapshots.contains_key(from) {
        return error(404, None);
    }
    f.made += 1;
    let id = format!("sbx{}", f.made);
    f.from.insert(id.clone(), from.to_owned());
    f.boxes.insert(id.clone(), ("running".into(), body["metadata"].clone()));
    f.rules.insert(id.clone(), body["network"]["rules"].clone());
    let made = json!({
        "sandboxID": id, "templateID": "rki5dems9wqfm4r03t7g", "alias": "base", "clientID": "x",
        "envdVersion": "0.6.10", "envdAccessToken": "envd-secret", "trafficAccessToken": "traffic-secret",
    });
    (StatusCode::CREATED, Json(made)).into_response()
}

async fn list(
    AxState(s): AxState<S>,
    m: Method,
    uri: Uri,
    h: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    if let Some(r) = record(&s, &m, &uri, Value::Null) {
        return r;
    }
    if !authed(&h) {
        return error(401, None);
    }
    let f = s.lock().unwrap();
    // E2B's parsing: the query's `metadata` is itself a query string whose
    // keys and values are encoded once more.
    let filter: Vec<(String, String)> = url::form_urlencoded::parse(q.get("metadata").map_or("", |m| m).as_bytes())
        .map(|(k, v)| {
            let dec = |s: &str| url::form_urlencoded::parse(format!("x={s}").as_bytes()).next().unwrap().1.into_owned();
            (dec(&k), dec(&v))
        })
        .collect();
    let states: Vec<&str> = q.get("state").map_or("running", |s| s).split(',').collect();
    let all: Vec<Value> = f
        .boxes
        .iter()
        .filter(|(_, (st, md))| states.contains(&st.as_str()) && filter.iter().all(|(k, v)| md[k] == *v.as_str()))
        .map(|(id, (st, md))| listed(id, st, md))
        .collect();
    let from: usize = q.get("nextToken").map_or(0, |t| t.parse().unwrap());
    let page = f.page.max(1);
    let end = (from + page).min(all.len());
    let mut r = Json(all[from..end].to_vec()).into_response();
    if end < all.len() {
        r.headers_mut().insert("x-next-token", end.to_string().parse().unwrap());
    }
    r
}

async fn one(AxState(s): AxState<S>, m: Method, uri: Uri, Path(id): Path<String>) -> Response {
    if let Some(r) = record(&s, &m, &uri, Value::Null) {
        return r;
    }
    let mut f = s.lock().unwrap();
    if m == Method::DELETE {
        return match f.boxes.remove(&id) {
            Some(_) => {
                if let Some(snap) = f.from.get(&id).filter(|t| f.snapshots.contains_key(*t)).cloned() {
                    let linger = f.linger;
                    f.lingering.insert(snap, linger);
                }
                StatusCode::NO_CONTENT.into_response()
            }
            None => error(404, None),
        };
    }
    match f.boxes.get(&id) {
        Some((st, md)) => {
            let mut d = listed(&id, st, md);
            d["lifecycle"] = json!({"onTimeout": "pause", "autoResume": false});
            d["envdAccessToken"] = json!("envd-secret");
            Json(d).into_response()
        }
        None => error(404, None),
    }
}

async fn action(
    AxState(s): AxState<S>,
    m: Method,
    uri: Uri,
    Path((id, what)): Path<(String, String)>,
    body: Option<Json<Value>>,
) -> Response {
    if let Some(r) = record(&s, &m, &uri, body.map_or(Value::Null, |b| b.0)) {
        return r;
    }
    let mut f = s.lock().unwrap();
    if what == "snapshots" && f.no_snapshots {
        return StatusCode::NOT_FOUND.into_response();
    }
    if what == "snapshots" && f.boxes.contains_key(&id) {
        f.snapped += 1;
        let snap = format!("snap{}:default", f.snapped);
        f.snapshots.insert(snap.clone(), id);
        return (StatusCode::CREATED, Json(json!({"snapshotID": snap, "names": []}))).into_response();
    }
    let Some((st, _)) = f.boxes.get_mut(&id) else { return error(404, None) };
    match what.as_str() {
        "pause" if st == "paused" => error(409, None),
        "pause" => {
            *st = "paused".into();
            StatusCode::NO_CONTENT.into_response()
        }
        // As E2B and loam: a paused box's timeout is a 404.
        "timeout" if st == "paused" => error(404, None),
        "timeout" => StatusCode::NO_CONTENT.into_response(),
        "connect" => {
            let woke = st == "paused";
            *st = "running".into();
            let code = if woke { StatusCode::CREATED } else { StatusCode::OK };
            (code, Json(json!({"sandboxID": id, "templateID": "base", "clientID": "x", "envdVersion": "0.6.10"})))
                .into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /events/sandboxes/{id}`: E2B's paging (`offset`, `limit`), its
/// `types` filter, and a 404 for a box it never had.
async fn events(AxState(s): AxState<S>, m: Method, uri: Uri, Path(id): Path<String>) -> Response {
    if let Some(r) = record(&s, &m, &uri, Value::Null) {
        return r;
    }
    let f = s.lock().unwrap();
    let Some(all) = f.events.get(&id) else { return error(404, None) };
    let q: Vec<(String, String)> =
        url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()).into_owned().collect();
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.parse::<usize>().unwrap());
    let types: Vec<&str> = q.iter().filter(|(n, _)| n == "types").map(|(_, v)| v.as_str()).collect();
    let kept: Vec<Value> = all.iter().filter(|e| types.contains(&e["type"].as_str().unwrap())).cloned().collect();
    let from = get("offset").unwrap_or(0).min(kept.len());
    let end = (from + get("limit").unwrap_or(10)).min(kept.len());
    Json(kept[from..end].to_vec()).into_response()
}

fn event(id: u32, kind: &str, run: Option<(&str, u64)>) -> Value {
    let mut data = json!({"sandbox_metadata": {"arugula_workspace": "hw_1"}});
    if let Some((started, ms)) = run {
        data["execution"] = json!({"execution_time": ms, "memory_mb": 2048, "started_at": started, "vcpu_count": 2});
    }
    json!({
        "id": format!("00000000-0000-0000-0000-{id:012}"), "version": "v2", "type": format!("sandbox.lifecycle.{kind}"),
        "timestamp": "2026-10-22T00:00:00Z", "sandboxId": "sbx1", "sandboxExecutionId": "x",
        "sandboxTemplateId": "t", "sandboxBuildId": "b", "sandboxTeamId": "00000000-0000-0000-0000-000000000000",
        "eventData": data,
    })
}

async fn network(
    AxState(s): AxState<S>,
    m: Method,
    uri: Uri,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Response {
    if let Some(r) = record(&s, &m, &uri, b.clone()) {
        return r;
    }
    let mut f = s.lock().unwrap();
    match f.boxes.get(&id) {
        // As E2B: a paused box's network can't be changed.
        Some((st, _)) if st == "running" => {
            f.rules.insert(id, b["rules"].clone());
            StatusCode::NO_CONTENT.into_response()
        }
        _ => error(404, None),
    }
}

/// `GET /snapshots` (M263), every one or a box's (`sandboxID`).
async fn snapshots(AxState(s): AxState<S>, m: Method, uri: Uri, Query(q): Query<BTreeMap<String, String>>) -> Response {
    if let Some(r) = record(&s, &m, &uri, Value::Null) {
        return r;
    }
    let f = s.lock().unwrap();
    if f.no_snapshots {
        return StatusCode::NOT_FOUND.into_response();
    }
    let of = q.get("sandboxID");
    let all: Vec<Value> = f
        .snapshots
        .iter()
        .filter(|(_, from)| of.is_none_or(|o| o == *from))
        .map(|(id, _)| json!({"snapshotID": id, "names": []}))
        .collect();
    Json(all).into_response()
}

/// `DELETE /templates/{id}`: a snapshot, as E2B deletes one.
async fn template(AxState(s): AxState<S>, m: Method, uri: Uri, Path(id): Path<String>) -> Response {
    if let Some(r) = record(&s, &m, &uri, Value::Null) {
        return r;
    }
    let mut f = s.lock().unwrap();
    if f.no_snapshots {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !f.snapshots.contains_key(&id) {
        return error(404, None);
    }
    if f.boxes.keys().any(|b| f.from.get(b) == Some(&id)) {
        return error(400, None);
    }
    if let Some(n) = f.lingering.get_mut(&id).filter(|n| **n > 0) {
        *n -= 1;
        return error(400, None);
    }
    f.snapshots.remove(&id);
    StatusCode::NO_CONTENT.into_response()
}

async fn fake() -> (E2b, S, String) {
    let s: S = Arc::new(Mutex::new(Fake { page: 100, ..Default::default() }));
    let app = Router::new()
        .route("/v2/sandboxes", post(create).get(list))
        .route("/sandboxes/{id}", any(one))
        .route("/sandboxes/{id}/network", put(network))
        .route("/sandboxes/{id}/{what}", post(action))
        .route("/v2/sandboxes/{id}/{what}", post(action))
        .route("/events/sandboxes/{id}", get(events))
        .route("/snapshots", get(snapshots))
        .route("/templates/{id}", axum::routing::delete(template))
        .route("/health", get(|| async { "ok" }))
        .with_state(s.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (E2b::new(&base, KEY.into()).unwrap(), s, base)
}

fn workspace(id: &str) -> Create {
    Create {
        template: "base".into(),
        env: [("ARUGULA_HOSTED_TICKET".into(), "t1".into())].into(),
        metadata: [("arugula_workspace".into(), id.into()), ("arugula_project".into(), "prj_1".into())].into(),
        timeout: Duration::from_secs(300),
        rules: Rules::new(),
    }
}

fn calls(s: &S) -> Vec<(String, String, Value)> {
    s.lock().unwrap().calls.clone()
}

#[tokio::test]
async fn create_pauses_on_timeout_and_never_auto_resumes() {
    let (e2b, s, _) = fake().await;
    let made = e2b.create(&workspace("hw_1")).await.unwrap();
    assert_eq!(made, Created { id: "sbx1".into(), template: "rki5dems9wqfm4r03t7g".into() });
    let (method, path, body) = calls(&s).remove(0);
    assert_eq!((method.as_str(), path.as_str()), ("POST", "/v2/sandboxes"));
    assert_eq!(body["autoPause"], true);
    assert_eq!(body["autoPauseMemory"], true);
    assert_eq!(body["autoResume"], json!({"enabled": false}));
    assert_eq!(body["secure"], true, "envd takes anyone in a box made without it");
    assert_eq!(body["network"]["allowPublicTraffic"], false, "E2B serves the box's ports to anyone without it");
    assert_eq!(body["timeout"], 300);
    assert_eq!(body["templateID"], "base");
    assert_eq!(body["envVars"], json!({"ARUGULA_HOSTED_TICKET": "t1"}));
    assert_eq!(body["metadata"], json!({"arugula_workspace": "hw_1", "arugula_project": "prj_1"}));
    assert!(body["network"].get("rules").is_none(), "no rules, no rules config");
    assert!(!body.to_string().contains(KEY), "the key goes in a header, never a body");
    let shown = format!("{made:?}{e2b:?}");
    assert!(!shown.contains("secret") && !shown.contains(KEY));
}

/// A create answered without an envd access token made a box whose envd
/// takes anyone: it's deleted and the create refused.
#[tokio::test]
async fn create_refuses_a_box_whose_envd_is_open() {
    let (e2b, s, _) = fake().await;
    for token in [Value::Null, json!("")] {
        let open = json!({"sandboxID": "sbx9", "templateID": "base", "clientID": "x", "envdVersion": "0.6.10",
            "envdAccessToken": token});
        s.lock().unwrap().failures.push_back((StatusCode::CREATED, Json(open)).into_response());
        assert_eq!(
            e2b.create(&workspace("hw_1")).await,
            Err(Error::Refused { status: 201, code: Some("envd_not_secured".into()) })
        );
        let (method, path, _) = calls(&s).pop().unwrap();
        assert_eq!((method.as_str(), path.as_str()), ("DELETE", "/sandboxes/sbx9"));
    }
    let open = json!({"sandboxID": "sbx9", "templateID": "base", "clientID": "x", "envdVersion": "0.6.10"});
    s.lock().unwrap().failures.push_back((StatusCode::CREATED, Json(open)).into_response());
    assert!(e2b.create(&workspace("hw_1")).await.is_err());
}

/// A box's runs are its pauses' and its delete's, read across pages, each
/// once: a delete of a paused box ends no run. A box E2B never had (or a
/// provider with no events, loam) has no record.
#[tokio::test]
async fn runs_are_read_from_lifecycle_events() {
    let (e2b, s, _) = fake().await;
    let mut evs = vec![event(0, "created", None)];
    // 120 pause-resume cycles, a second each: two pages of runs.
    for i in 0..120u32 {
        evs.push(event(2 * i + 1, "paused", Some(("2026-10-22T00:00:00Z", 1_000 + u64::from(i)))));
        evs.push(event(2 * i + 2, "resumed", None));
    }
    evs.push(event(999, "killed", Some(("2026-10-22T01:00:00.5Z", 400))));
    evs.push(event(1000, "killed", None));
    s.lock().unwrap().events.insert("sbx1".into(), evs);
    let runs = e2b.runs("sbx1").await.unwrap().unwrap();
    assert_eq!(runs.len(), 121);
    assert_eq!(runs[0], Run { started_at: 1_792_627_200_000, ms: 1_000, cpus: 2, memory_mb: 2048 });
    assert_eq!(runs[120], Run { started_at: 1_792_630_800_500, ms: 400, cpus: 2, memory_mb: 2048 });
    let paths: Vec<String> = calls(&s).into_iter().map(|c| c.1).filter(|p| p.starts_with("/events/")).collect();
    assert_eq!(paths.len(), 2, "{paths:?}");
    assert!(paths[1].contains("offset=100") && paths[1].contains("orderAsc=true"), "{}", paths[1]);
    assert!(paths[0].contains("types=sandbox.lifecycle.paused&types=sandbox.lifecycle.killed"), "{}", paths[0]);
    assert_eq!(e2b.runs("nope").await.unwrap(), None);
}

#[tokio::test]
async fn status_reads_without_waking() {
    let (e2b, s, _) = fake().await;
    let id = e2b.create(&workspace("hw_1")).await.unwrap().id;
    assert_eq!(e2b.pause(&id).await.unwrap(), Paused::Paused);
    let b = e2b.status(&id).await.unwrap().unwrap();
    assert_eq!((b.state, b.pauses_on_timeout), (State::Paused, Some(true)));
    assert_eq!((b.cpus, b.memory_mb), (Some(2), Some(512)));
    assert_eq!(b.metadata["arugula_workspace"], "hw_1");
    assert_eq!(e2b.status(&id).await.unwrap().unwrap().state, State::Paused);
    assert!(calls(&s).iter().all(|(_, p, _)| !p.contains("connect") && !p.contains("resume")));
    assert_eq!(e2b.status("nope").await.unwrap(), None);
}

#[tokio::test]
async fn pause_resume_hold_delete() {
    let (e2b, s, _) = fake().await;
    let id = e2b.create(&workspace("hw_1")).await.unwrap().id;
    assert_eq!(e2b.resume(&id, Duration::from_secs(60)).await.unwrap(), Resumed::Running);
    e2b.hold(&id, Duration::from_secs(120)).await.unwrap();
    assert_eq!(e2b.pause(&id).await.unwrap(), Paused::Paused);
    assert_eq!(e2b.pause(&id).await.unwrap(), Paused::Already);
    assert_eq!(e2b.hold(&id, Duration::from_secs(120)).await, Err(Error::NotFound), "hold never wakes");
    assert_eq!(e2b.resume(&id, Duration::from_secs(60)).await.unwrap(), Resumed::Woke);
    e2b.delete(&id).await.unwrap();
    e2b.delete(&id).await.unwrap();
    assert_eq!(e2b.status(&id).await.unwrap(), None);
    assert_eq!(e2b.pause(&id).await, Err(Error::NotFound));
    let paths = calls(&s);
    let find = |want: &str| paths.iter().find(|(_, p, _)| p.ends_with(want)).unwrap().clone();
    assert_eq!(find("/connect").1, format!("/v2/sandboxes/{id}/connect"));
    assert_eq!(find("/connect").2, json!({"timeout": 60}));
    assert_eq!(find("/timeout").2, json!({"timeout": 120}));
    assert_eq!(find("/pause").2, json!({}));
}

#[tokio::test]
async fn list_filters_by_metadata_across_pages() {
    let (e2b, s, _) = fake().await;
    s.lock().unwrap().page = 2;
    for i in 0..5 {
        e2b.create(&workspace(&format!("hw_{}", i % 2))).await.unwrap();
    }
    // An awkward value survives E2B's double encoding.
    let mut odd = workspace("hw_&= /x");
    odd.metadata.insert("arugula_release".into(), "0.58.0+a b".into());
    let odd_id = e2b.create(&odd).await.unwrap().id;
    e2b.pause("sbx1").await.unwrap();

    let want: BTreeMap<String, String> = [("arugula_workspace".into(), "hw_0".into())].into();
    let mut ids: Vec<_> = e2b.list(&want).await.unwrap().into_iter().map(|b| (b.id, b.state)).collect();
    ids.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(ids, [("sbx1".into(), State::Paused), ("sbx3".into(), State::Running), ("sbx5".into(), State::Running)]);
    let lists: Vec<_> = calls(&s).into_iter().filter(|(m, p, _)| m == "GET" && p.starts_with("/v2/")).collect();
    assert!(lists.len() >= 2, "followed X-Next-Token: {lists:?}");
    assert!(lists.iter().all(|(_, p, _)| p.contains("state=running%2Cpaused")));

    let got = e2b.list(&odd.metadata).await.unwrap();
    assert_eq!(got.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), [odd_id.as_str()]);
    assert_eq!(e2b.list(&BTreeMap::new()).await.unwrap().len(), 6);
}

#[tokio::test]
async fn errors_keep_only_status_and_code() {
    let (e2b, s, base) = fake().await;
    let fail = |r: Response| s.lock().unwrap().failures.push_back(r);

    fail(error(503, Some("sandbox_capacity_unavailable")));
    let e = e2b.create(&workspace("hw_1")).await.unwrap_err();
    assert_eq!(
        e,
        Error::Unavailable { status: 503, code: Some("sandbox_capacity_unavailable".into()), retry_after: None }
    );
    assert!(!e.to_string().contains("secret") && !format!("{e:?}").contains("secret"));
    assert_eq!(calls(&s).len(), 1, "a create is never resent");

    fail(error(400, Some("Not A Code: ARUGULA_HOSTED_TICKET")));
    assert_eq!(e2b.create(&workspace("hw_1")).await, Err(Error::Refused { status: 400, code: None }));

    let mut unknown = workspace("hw_1");
    unknown.template = "nope".into();
    assert_eq!(e2b.create(&unknown).await, Err(Error::Refused { status: 404, code: None }));

    let bad = E2b::new(&base, "wrong".into()).unwrap();
    assert_eq!(bad.list(&BTreeMap::new()).await, Err(Error::Refused { status: 401, code: None }));

    // Idempotent calls ride out a 429 and a 502, and give up after three.
    let id = e2b.create(&workspace("hw_1")).await.unwrap().id;
    let mut busy = error(429, None);
    busy.headers_mut().insert("retry-after", "0".parse().unwrap());
    fail(busy);
    fail(error(502, None));
    assert_eq!(e2b.pause(&id).await.unwrap(), Paused::Paused);
    for _ in 0..3 {
        fail(error(503, None));
    }
    assert_eq!(e2b.status(&id).await, Err(Error::Unavailable { status: 503, code: None, retry_after: None }));

    fail((StatusCode::OK, "not json").into_response());
    assert_eq!(e2b.status(&id).await, Err(Error::Invalid));
}

/// M185: a box made with rules carries them as E2B's network rules, a
/// key or a broker reference alike, and a running box's rules are
/// replaced (a paused one's can't be). A create's `Debug` never shows a
/// rule's value.
#[tokio::test]
async fn rules() {
    let (e2b, s, _) = fake().await;
    let mut b = workspace("hw_1");
    let rules: Rules = [(
        "api.anthropic.com".to_owned(),
        [("x-api-key".to_owned(), "sk-ant-fake-value".to_owned())].into(), // gitleaks:allow: a test value
    )]
    .into();
    b.rules = rules.clone();
    assert!(!format!("{b:?}").contains("sk-ant"), "{b:?}");
    let id = e2b.create(&b).await.unwrap().id;
    let want = json!({"api.anthropic.com": [{"transform": {"headers": {"x-api-key": "sk-ant-fake-value"}}}]}); // gitleaks:allow
    assert_eq!(calls(&s)[0].2["network"], json!({"allowPublicTraffic": false, "rules": want}));

    let both: Rules = [
        ("api.anthropic.com".to_owned(), [("x-api-key".to_owned(), fill("arugula-platform-anthropic"))].into()),
        ("api.openai.com".to_owned(), [("authorization".to_owned(), format!("Bearer {}", fill("o")))].into()),
    ]
    .into();
    e2b.set_rules(&id, &both).await.unwrap();
    let (method, path, body) = calls(&s).pop().unwrap();
    assert_eq!((method.as_str(), path), ("PUT", format!("/sandboxes/{id}/network")));
    assert_eq!(body["rules"]["api.openai.com"][0]["transform"]["headers"]["authorization"], "Bearer ${e2b.secrets.o}");
    assert_eq!(
        body["rules"]["api.anthropic.com"][0]["transform"]["headers"]["x-api-key"],
        "${e2b.secrets.arugula-platform-anthropic}"
    );
    e2b.set_rules(&id, &Rules::new()).await.unwrap();
    assert_eq!(s.lock().unwrap().rules[&id], json!({}), "no rules clears them");
    e2b.pause(&id).await.unwrap();
    assert_eq!(e2b.set_rules(&id, &both).await, Err(Error::NotFound), "a paused box's rules can't change");
}

#[tokio::test]
async fn no_answer_is_unreachable() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    drop(l);
    let e2b = E2b::new(&base, KEY.into()).unwrap();
    assert_eq!(e2b.create(&workspace("hw_1")).await, Err(Error::Unreachable));
    assert_eq!(e2b.delete("sbx1").await, Err(Error::Unreachable));
}

#[test]
fn flags_and_urls() {
    use clap::Parser;
    #[derive(clap::Parser)]
    struct T {
        #[command(flatten)]
        compute: Args,
    }
    let off = T::try_parse_from(["t"]).unwrap().compute;
    assert_eq!(off.e2b_url, "https://api.e2b.dev");
    assert!(off.substrate().unwrap().is_none());
    let on = T::try_parse_from(["t", "--e2b-api-key", "k", "--e2b-url", "https://api.e2b.example"]).unwrap();
    assert!(on.compute.substrate().unwrap().is_some());
    for base in ["http://e2b.example", "https://user:pw@e2b.example", "https://e2b.example?key=x"] {
        assert!(E2b::new(base, "k".into()).is_err(), "{base}");
    }
    assert!(E2b::new("https://api.e2b.dev", " ".into()).is_err());
}

// ---------------------------------------------------------------- snapshots (M263)

/// A snapshot of a running box is filesystem-only or full, as asked; a box
/// is made from it as from a template; a box's are listed; a delete is
/// fine twice. The source keeps running.
#[tokio::test]
async fn snapshots_are_made_used_listed_and_deleted() {
    let (e2b, s, _) = fake().await;
    assert!(e2b.snapshots().await.unwrap());
    let id = e2b.create(&workspace("hw_1")).await.unwrap().id;
    let snap = e2b.snapshot(&id, Mode::Filesystem).await.unwrap();
    assert_eq!(snap, "snap1:default");
    let (method, path, body) = calls(&s).pop().unwrap();
    assert_eq!((method.as_str(), path), ("POST", format!("/sandboxes/{id}/snapshots")));
    assert_eq!(body, json!({"memory": false}));
    e2b.snapshot(&id, Mode::Full).await.unwrap();
    assert_eq!(calls(&s).pop().unwrap().2, json!({"memory": true}));
    assert_eq!(e2b.status(&id).await.unwrap().unwrap().state, State::Running, "the source runs on");

    let mut child = workspace("hw_1");
    child.template = snap.clone();
    let made = e2b.create(&child).await.unwrap();
    assert_eq!(calls(&s).pop().unwrap().2["templateID"], snap);
    assert_ne!(made.id, id);

    let mut of = e2b.list_snapshots(Some(&id)).await.unwrap();
    of.sort();
    assert_eq!(of, ["snap1:default", "snap2:default"]);
    assert!(calls(&s).pop().unwrap().1.contains(&format!("sandboxID={id}")));
    assert!(e2b.list_snapshots(Some(&made.id)).await.unwrap().is_empty());
    // As E2B: not while a box made from it exists, paused or not, nor
    // just after it's deleted.
    s.lock().unwrap().linger = 1;
    e2b.pause(&made.id).await.unwrap();
    assert_eq!(e2b.delete_snapshot(&snap).await, Err(Error::Refused { status: 400, code: None }));
    e2b.delete(&made.id).await.unwrap();
    assert_eq!(e2b.delete_snapshot(&snap).await, Err(Error::Refused { status: 400, code: None }));
    e2b.delete_snapshot(&snap).await.unwrap();
    assert_eq!(calls(&s).pop().unwrap().1, "/templates/snap1:default");
    e2b.delete_snapshot(&snap).await.unwrap();
    assert_eq!(e2b.list_snapshots(None).await.unwrap(), ["snap2:default"]);
    // A box from a snapshot that's gone isn't made.
    assert_eq!(e2b.create(&child).await, Err(Error::Refused { status: 404, code: None }));
    // No such box.
    assert_eq!(e2b.snapshot("nope", Mode::Filesystem).await, Err(Error::NotFound));
}

/// loam's E2B API answers 404 for every snapshot route: there are none,
/// and a snapshot of a box it has is unsupported, not "no such box".
#[tokio::test]
async fn a_provider_without_snapshots_says_so() {
    let (e2b, s, _) = fake().await;
    s.lock().unwrap().no_snapshots = true;
    assert!(!e2b.snapshots().await.unwrap());
    let id = e2b.create(&workspace("hw_1")).await.unwrap().id;
    assert_eq!(e2b.snapshot(&id, Mode::Filesystem).await, Err(Error::Unsupported));
    assert_eq!(e2b.snapshot("nope", Mode::Filesystem).await, Err(Error::NotFound));
    assert_eq!(e2b.list_snapshots(None).await, Err(Error::Unsupported));
    e2b.delete_snapshot("snap1:default").await.unwrap();
}

mod lifecycle {
    //! The platform's snapshots through its worker and routes, on the E2B
    //! adapter against the fake: building one, a box from it, a spec change
    //! building another, a failure keeping the current one, and deleting.

    use std::sync::Arc;

    use arugula_control_wire::hosted::{FROM_SNAPSHOT, SEALED, SetupReport, SetupStatus};
    use arugula_e2e::now_ms;
    use serde_json::{Value, json};

    use super::{S, fake};
    use crate::{
        Platform, PlatformPort,
        chat_tests::{TestHub, call},
        hosted::{
            EnrollBy, snapshot,
            tests::{drive, make, serve_on, show},
        },
    };

    async fn on_fake() -> (Arc<TestHub>, S, Arc<Platform>, String, String) {
        let (e2b, s, _) = fake().await;
        let (hub, app, base, project) = serve_on(EnrollBy::Sandbox, Arc::new(e2b), "base").await;
        hub.delegate(&project);
        (hub, s, app, base, project)
    }

    fn said(status: SetupStatus, step: &str, reason: &str) -> SetupReport {
        SetupReport { status, step: step.into(), reason: reason.into(), pane: Some(1), spec_hash: String::new() }
    }

    /// The boxes the fake has: each one's `templateID` and workspace.
    fn boxes(s: &S) -> Vec<(String, String)> {
        let f = s.lock().unwrap();
        let mut made: Vec<(String, String)> = f
            .calls
            .iter()
            .filter(|(m, p, _)| m == "POST" && p == "/v2/sandboxes")
            .map(|(_, _, b)| {
                (
                    b["templateID"].as_str().unwrap().to_owned(),
                    b["metadata"]["arugula_workspace"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        made.dedup();
        made
    }

    fn live(s: &S) -> usize {
        s.lock().unwrap().boxes.len()
    }

    fn snapshots(s: &S) -> Vec<String> {
        s.lock().unwrap().snapshots.keys().cloned().collect()
    }

    /// Workspace `ws`'s box enrolls as `machine` and says it's set up, as a
    /// box set up by its setup (`step` `"Set up."`) or from a snapshot.
    async fn set_up(app: &Platform, hub: &TestHub, project: &str, ws: &str, machine: &str, step: &str, now: u64) {
        hub.enroll(ws, machine);
        drive(app, now).await;
        assert!(app.hosted_setup(project, ws, machine, &said(SetupStatus::Ready, step, "")).await.unwrap());
    }

    /// The worker looks at `ws` now: a build starts when one is due. Its
    /// builder's id, if it has one.
    async fn look(app: &Platform, ws: &str, now: u64) -> Option<String> {
        app.db.nudge_hosted(ws).unwrap();
        drive(app, now).await;
        app.db.snapshot_builder(ws).unwrap().map(|b| b.id)
    }

    /// Builder `b` enrolls as `machine`, sets up and seals, and its daemon
    /// goes off the relay: it's snapshotted and deleted.
    async fn seal(app: &Platform, hub: &TestHub, project: &str, b: &str, machine: &str, now: u64) {
        hub.enroll(b, machine);
        drive(app, now).await;
        assert!(app.hosted_setup(project, b, machine, &said(SetupStatus::SettingUp, "clone app", "")).await.unwrap());
        assert!(app.hosted_setup(project, b, machine, &said(SetupStatus::Ready, SEALED, "")).await.unwrap());
        hub.set_online(machine, false);
        drive(app, now).await;
    }

    fn snap(w: &Value) -> &Value {
        &w["snapshot"]
    }

    /// Made, used, rebuilt on a spec change, kept through a failure, and
    /// deleted with its workspace.
    #[tokio::test]
    async fn a_workspace_snapshot_is_built_used_rebuilt_and_deleted() {
        let (hub, s, app, base, project) = on_fake().await;
        let (_, w) = make(&base, "alice", &project, "dev").await;
        let ws = w["id"].as_str().unwrap().to_owned();
        let path = format!("/api/projects/{project}/workspaces/{ws}");
        let (status, _) =
            call(&base, "alice", "PUT", &format!("{path}/secrets/GIT_TOKEN"), Some(json!({ "value": "tok-secret-1" })))
                .await;
        assert_eq!(status, 204);
        let now = now_ms();
        drive(&app, now).await;
        assert!(look(&app, &ws, now).await.is_none(), "not before its box set up");
        set_up(&app, &hub, &project, &ws, "m1", "Set up.", now + 3_000).await;
        assert!(show(&base, &project, &ws).await.get("snapshot").is_none());

        // Create: a builder, its own box from the template, enrolled for
        // purpose snapshot, reading the workspace's plan and secrets.
        let b = look(&app, &ws, now + 3_000).await.expect("a build once it set up");
        assert_eq!(snap(&show(&base, &project, &ws).await)["status"], "building");
        assert_eq!(boxes(&s), [("base".to_owned(), ws.clone()), ("base".to_owned(), b.clone())]);
        let asked = hub.enrollments();
        let ask = asked.iter().find(|a| a.workspace == b).unwrap();
        assert_eq!(ask.purpose.as_deref(), Some("snapshot"));
        let builder_box = s
            .lock()
            .unwrap()
            .calls
            .iter()
            .rev()
            .find(|(m, p, _)| m == "POST" && p == "/v2/sandboxes")
            .unwrap()
            .2
            .clone();
        assert_eq!(builder_box["timeout"], snapshot::BUILD_TIMEOUT.as_secs());
        assert_eq!(builder_box["network"].get("rules"), None, "a builder runs no agents: no key");
        assert!(look(&app, &ws, now + 3_000).await.is_some_and(|again| again == b), "one build at a time");
        let listed = call(&base, "alice", "GET", &format!("/api/projects/{project}/workspaces"), None).await.1;
        assert_eq!(listed["workspaces"].as_array().unwrap().len(), 1, "nobody lists a builder: {listed}");
        hub.enroll(&b, "b1");
        drive(&app, now + 6_000).await;
        let plan = app.hosted_plan(&project, &b, "b1").await.unwrap().expect("the builder's plan");
        assert_eq!(plan.repos[0].password.as_deref(), Some("tok-secret-1"), "the workspace's secrets");
        assert!(app.hosted_plan(&project, &b, "someone-else").await.unwrap().is_none());

        // Sealed, but its daemon is still on the relay: not yet.
        assert!(app.hosted_setup(&project, &b, "b1", &said(SetupStatus::Ready, SEALED, "")).await.unwrap());
        drive(&app, now + 6_000).await;
        assert!(snapshots(&s).is_empty(), "the daemon hasn't stopped");
        hub.set_online("b1", false);
        drive(&app, now + 7_000).await;
        assert_eq!(snapshots(&s), ["snap1:default"]);
        let took = s.lock().unwrap().calls.iter().find(|(_, p, _)| p.ends_with("/snapshots")).unwrap().2.clone();
        assert_eq!(took, json!({"memory": false}), "filesystem-only");
        assert_eq!(live(&s), 1, "the builder's box is gone");
        assert!(app.db.snapshot_builder(&ws).unwrap().is_none());
        assert!(hub.gone().contains(&b));
        let w = show(&base, &project, &ws).await;
        assert_eq!(snap(&w)["status"], "ready");
        assert!(snap(&w)["built_at"].as_u64().unwrap() >= now);
        assert!(!w.to_string().contains("snap1"), "the snapshot's id isn't answered: {w}");

        // Use: a rebuild starts from it, and its box says it's set up.
        call(&base, "alice", "POST", &format!("{path}/rebuild"), None).await;
        drive(&app, now + 8_000).await;
        assert_eq!(boxes(&s).last().unwrap(), &("snap1:default".to_owned(), ws.clone()));
        let made = s
            .lock()
            .unwrap()
            .calls
            .iter()
            .rev()
            .find(|(m, p, _)| m == "POST" && p == "/v2/sandboxes")
            .unwrap()
            .2
            .clone();
        assert_eq!(made["metadata"]["arugula_snapshot"], "snap1:default");
        assert_eq!(made["metadata"]["arugula_template"], "base");
        set_up(&app, &hub, &project, &ws, "m2", FROM_SNAPSHOT, now + 11_000).await;
        let w = show(&base, &project, &ws).await;
        assert_eq!((w["state"].as_str(), w["setup"]["step"].as_str()), (Some("ready"), Some(FROM_SNAPSHOT)));
        assert_eq!(w["rebuild_needed"], false);
        assert!(look(&app, &ws, now + 11_000).await.is_none(), "nothing's due");

        // A spec change: a rebuild before the next snapshot starts from the
        // template, and the next build starts at once.
        let spec = json!({ "repos": ["https://github.com/acme/app"], "setup": "make all" });
        assert_eq!(call(&base, "alice", "PUT", &format!("{path}/spec"), Some(spec)).await.0, 200);
        call(&base, "alice", "POST", &format!("{path}/rebuild"), None).await;
        drive(&app, now + 12_000).await;
        assert_eq!(boxes(&s).last().unwrap(), &("base".to_owned(), ws.clone()), "the snapshot is the old spec's");
        set_up(&app, &hub, &project, &ws, "m3", "Set up.", now + 15_000).await;
        let b2 = look(&app, &ws, now + 15_000).await.expect("a build for the new spec");
        seal(&app, &hub, &project, &b2, "b2", now + 18_000).await;
        assert_eq!(snapshots(&s), ["snap2:default"], "the one before was deleted: no box is made from it");
        assert_eq!(snap(&show(&base, &project, &ws).await)["status"], "ready");

        // The one before stays while the box made from it lives.
        call(&base, "alice", "POST", &format!("{path}/rebuild"), None).await;
        drive(&app, now + 19_000).await;
        assert_eq!(boxes(&s).last().unwrap(), &("snap2:default".to_owned(), ws.clone()));
        set_up(&app, &hub, &project, &ws, "m4", FROM_SNAPSHOT, now + 22_000).await;
        let day = 24 * 3600 * 1000;
        // A day after the last try (b2's, at +15 s).
        let day = day + 30_000;
        assert!(look(&app, &ws, now + day - 20_000).await.is_none(), "not quite a day");
        let b3 = look(&app, &ws, now + day).await.expect("a day on, built again");
        seal(&app, &hub, &project, &b3, "b3", now + day + 3_000).await;
        assert_eq!(snapshots(&s), ["snap2:default", "snap3:default"], "snap2 is its box's");
        call(&base, "alice", "POST", &format!("{path}/rebuild"), None).await;
        drive(&app, now + day + 4_000).await;
        assert_eq!(boxes(&s).last().unwrap(), &("snap3:default".to_owned(), ws.clone()));
        set_up(&app, &hub, &project, &ws, "m5", FROM_SNAPSHOT, now + day + 7_000).await;
        // E2B holds snap2 a while after its box went: a deferred delete,
        // asked at each look, never the build's error.
        s.lock().unwrap().lingering.insert("snap2:default".into(), 1);
        look(&app, &ws, now + day + 7_000).await;
        assert_eq!(snapshots(&s), ["snap2:default", "snap3:default"], "still held");
        let w = show(&base, &project, &ws).await;
        assert_eq!(snap(&w)["status"], "ready");
        assert!(snap(&w).get("error").is_none(), "{w}");
        look(&app, &ws, now + day + 8_000).await;
        assert_eq!(snapshots(&s), ["snap3:default"], "deleted once its box went");

        // A failure keeps the current one, and says why.
        let b4 = look(&app, &ws, now + 2 * day).await.expect("another day");
        hub.enroll(&b4, "b4");
        drive(&app, now + 2 * day + 3_000).await;
        let r = said(SetupStatus::Failed, "setup script", "exit");
        assert!(app.hosted_setup(&project, &b4, "b4", &r).await.unwrap());
        drive(&app, now + 2 * day + 3_000).await;
        let w = show(&base, &project, &ws).await;
        assert_eq!(snap(&w)["status"], "failed");
        assert!(snap(&w)["error"].as_str().unwrap().contains("setup script"), "{w}");
        assert!(snap(&w)["built_at"].as_u64().is_some(), "the current one stays");
        assert_eq!(snapshots(&s), ["snap3:default"]);
        assert!(app.db.snapshot_builder(&ws).unwrap().is_none(), "the failed builder went");
        assert!(look(&app, &ws, now + 2 * day + 4_000).await.is_none(), "not again until it's due");
        call(&base, "alice", "POST", &format!("{path}/rebuild"), None).await;
        drive(&app, now + 2 * day + 5_000).await;
        assert_eq!(boxes(&s).last().unwrap(), &("snap3:default".to_owned(), ws.clone()), "still used");

        // A builder whose wipe found a secret left: no snapshot either.
        set_up(&app, &hub, &project, &ws, "m6", FROM_SNAPSHOT, now + 3 * day).await;
        let b5 = look(&app, &ws, now + 3 * day).await.unwrap();
        hub.enroll(&b5, "b5");
        drive(&app, now + 3 * day + 3_000).await;
        assert!(app.hosted_setup(&project, &b5, "b5", &said(SetupStatus::Failed, SEALED, "seal_left")).await.unwrap());
        drive(&app, now + 3 * day + 3_000).await;
        let w = show(&base, &project, &ws).await;
        assert!(snap(&w)["error"].as_str().unwrap().contains("still on the snapshot's box"), "{w}");
        assert_eq!(snapshots(&s), ["snap3:default"]);

        // Delete: its builder and boxes go, then every snapshot, once E2B
        // lets go of the one its box was made from: until then it stays
        // `deleting`, and asks again.
        let b6 = look(&app, &ws, now + 4 * day).await.unwrap();
        s.lock().unwrap().linger = 2;
        assert_eq!(call(&base, "alice", "DELETE", &path, None).await.0, 200);
        drive(&app, now + 4 * day + 1_000).await;
        assert_eq!(snapshots(&s), ["snap3:default"], "held just after its box went");
        assert_eq!(live(&s), 0, "its boxes went first");
        let state = |app: &Platform| -> String {
            app.db.hosted_any(&project, &ws).unwrap().map(|w| w.state).unwrap_or_default()
        };
        assert_eq!(state(&app), "deleting");
        drive(&app, now + 4 * day + 20_000).await;
        assert_eq!(state(&app), "deleting");
        drive(&app, now + 4 * day + 40_000).await;
        assert!(snapshots(&s).is_empty(), "{:?}", snapshots(&s));
        assert_eq!(state(&app), "deleted");
        assert_eq!(live(&s), 0, "every box went");
        assert!(app.db.snapshot_builder(&ws).unwrap().is_none());
        assert!(hub.gone().contains(&b6));
    }

    /// A builder whose daemon doesn't seal (an older template), and a
    /// snapshot the provider lost: no snapshot, and boxes from the template.
    #[tokio::test]
    async fn an_old_daemon_and_a_lost_snapshot_fall_back_to_the_template() {
        let (hub, s, app, base, project) = on_fake().await;
        let (_, w) = make(&base, "alice", &project, "dev").await;
        let ws = w["id"].as_str().unwrap().to_owned();
        let path = format!("/api/projects/{project}/workspaces/{ws}");
        let now = now_ms();
        drive(&app, now).await;
        set_up(&app, &hub, &project, &ws, "m1", "Set up.", now + 3_000).await;
        let b = look(&app, &ws, now + 3_000).await.unwrap();
        hub.enroll(&b, "b1");
        drive(&app, now + 6_000).await;
        assert!(app.hosted_setup(&project, &b, "b1", &said(SetupStatus::Ready, "Set up.", "")).await.unwrap());
        drive(&app, now + 6_000).await;
        let w = show(&base, &project, &ws).await;
        assert_eq!(snap(&w)["error"], snapshot::NO_SEAL);
        assert!(snapshots(&s).is_empty());

        let b = look(&app, &ws, now + 25 * 3600 * 1000).await.unwrap();
        seal(&app, &hub, &project, &b, "b2", now + 25 * 3600 * 1000 + 3_000).await;
        assert_eq!(snapshots(&s), ["snap1:default"]);
        s.lock().unwrap().snapshots.clear();
        call(&base, "alice", "POST", &format!("{path}/rebuild"), None).await;
        drive(&app, now + 25 * 3600 * 1000 + 4_000).await;
        let made = boxes(&s);
        assert_eq!(made[made.len() - 2..], [("snap1:default".to_owned(), ws.clone()), ("base".to_owned(), ws.clone())]);
        let w = show(&base, &project, &ws).await;
        assert_eq!(
            (w["state"].as_str(), snap(&w)["error"].as_str()),
            (Some("enrolling"), Some(snapshot::SNAPSHOT_GONE))
        );
        assert!(snap(&w)["built_at"].is_null());
    }

    /// loam's E2B API takes no snapshots: no builder is ever made, and it's
    /// asked once per interval, not at every look.
    #[tokio::test]
    async fn no_builds_where_the_provider_takes_no_snapshots() {
        let (hub, s, app, base, project) = on_fake().await;
        s.lock().unwrap().no_snapshots = true;
        let (_, w) = make(&base, "alice", &project, "dev").await;
        let ws = w["id"].as_str().unwrap().to_owned();
        let now = now_ms();
        drive(&app, now).await;
        set_up(&app, &hub, &project, &ws, "m1", "Set up.", now + 3_000).await;
        assert!(look(&app, &ws, now + 3_000).await.is_none());
        assert!(look(&app, &ws, now + 70_000).await.is_none());
        let asked = s.lock().unwrap().calls.iter().filter(|(_, p, _)| p.starts_with("/snapshots")).count();
        assert_eq!(asked, 1);
        assert_eq!(boxes(&s).len(), 1);
        assert!(show(&base, &project, &ws).await.get("snapshot").is_none());
    }
}
