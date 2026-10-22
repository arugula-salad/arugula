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
    routing::{any, get, post},
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
    if body["templateID"] != "base" {
        return error(404, None);
    }
    let mut f = s.lock().unwrap();
    f.made += 1;
    let id = format!("sbx{}", f.made);
    f.boxes.insert(id.clone(), ("running".into(), body["metadata"].clone()));
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
            Some(_) => StatusCode::NO_CONTENT.into_response(),
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

async fn fake() -> (E2b, S, String) {
    let s: S = Arc::new(Mutex::new(Fake { page: 100, ..Default::default() }));
    let app = Router::new()
        .route("/v2/sandboxes", post(create).get(list))
        .route("/sandboxes/{id}", any(one))
        .route("/sandboxes/{id}/{what}", post(action))
        .route("/v2/sandboxes/{id}/{what}", post(action))
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
    assert_eq!(body["timeout"], 300);
    assert_eq!(body["templateID"], "base");
    assert_eq!(body["envVars"], json!({"ARUGULA_HOSTED_TICKET": "t1"}));
    assert_eq!(body["metadata"], json!({"arugula_workspace": "hw_1", "arugula_project": "prj_1"}));
    assert!(!body.to_string().contains(KEY), "the key goes in a header, never a body");
    let shown = format!("{made:?}{e2b:?}");
    assert!(!shown.contains("secret") && !shown.contains(KEY));
}

#[tokio::test]
async fn status_reads_without_waking() {
    let (e2b, s, _) = fake().await;
    let id = e2b.create(&workspace("hw_1")).await.unwrap().id;
    assert_eq!(e2b.pause(&id).await.unwrap(), Paused::Paused);
    let b = e2b.status(&id).await.unwrap().unwrap();
    assert_eq!((b.state, b.pauses_on_timeout), (State::Paused, Some(true)));
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
