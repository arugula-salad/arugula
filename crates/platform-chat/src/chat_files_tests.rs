//! Files and images in chat (#107): who can fetch one, what it is served
//! as, the limits, and what takes it away.

use arugula_control_wire::chat::MAX_FILE_BYTES;
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::projects::{THREADS_POST, THREADS_READ},
};

const ROOM: &str = "/api/chat/rooms/ch_general";

/// A 2x3 PNG's head: enough for the sniffer, and for a browser to refuse.
fn png() -> Vec<u8> {
    [b"\x89PNG\r\n\x1a\n".as_slice(), &[0, 0, 0, 13], b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 3], &[8; 40]].concat()
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

async fn put(base: &str, who: &str, room: &str, name: &str, body: Vec<u8>) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/chat/rooms/{room}/files?name={}", enc(name)))
        .header("cookie", format!("ilg_session=session-{who}"))
        // What a hostile client says it is doesn't matter.
        .header("content-type", "text/html")
        .body(body)
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

async fn fetch(base: &str, who: &str, path: &str, range: Option<&str>) -> reqwest::Response {
    let mut r =
        reqwest::Client::new().get(format!("{base}{path}")).header("cookie", format!("ilg_session=session-{who}"));
    if let Some(v) = range {
        r = r.header("range", v);
    }
    r.send().await.unwrap()
}

async fn say_with(base: &str, who: &str, text: &str, files: &[&Value]) -> (u16, Value) {
    let ids: Vec<&Value> = files.iter().map(|f| &f["id"]).collect();
    call(base, who, "POST", &format!("{ROOM}/messages"), Some(json!({ "text": text, "files": ids }))).await
}

/// A picture is uploaded, posted, and the room reads it inline; someone
/// outside the room (another team's member, a DM's outsider) gets 404.
#[tokio::test]
async fn only_the_rooms_readers_fetch_a_file() {
    let (hub, _app, base, _t, _u) = seeded().await;
    // Chat is on for c too: it is the room that isn't theirs, not the switch.
    hub.flag("team:u", true);
    let (st, f) = put(&base, "a", "ch_general", "shot.png", png()).await;
    assert_eq!(st, 200, "{f}");
    assert_eq!(
        (f["kind"].clone(), f["mime"].clone(), f["width"].clone(), f["height"].clone()),
        (json!("image"), json!("image/png"), json!(2), json!(3))
    );
    let url = f["url"].as_str().unwrap().to_owned();

    // Before it's posted it is the uploader's alone.
    assert_eq!(fetch(&base, "a", &url, None).await.status(), 200);
    assert_eq!(fetch(&base, "b", &url, None).await.status(), 404);

    let (st, p) = say_with(&base, "a", "", &[&f]).await;
    assert_eq!(st, 200, "a message may be only a file: {p}");
    let (_, v) = call(&base, "b", "GET", &format!("{ROOM}/messages"), None).await;
    let m = &v["messages"][0];
    assert_eq!((m["files"][0]["name"].clone(), m["files"][0]["url"].clone()), (json!("shot.png"), json!(url)));

    let r = fetch(&base, "b", &url, None).await;
    assert_eq!(r.status(), 200);
    let h = r.headers().clone();
    assert_eq!(h["content-type"], "image/png");
    assert!(h["content-disposition"].to_str().unwrap().starts_with("inline"));
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h["content-security-policy"].to_str().unwrap().contains("default-src 'none'"));
    assert_eq!(r.bytes().await.unwrap().to_vec(), png());

    // Not in team t at all, and a DM's outsider: the same 404 as for no file.
    assert_eq!(fetch(&base, "c", &url, None).await.status(), 404);
    let none = format!("{ROOM}/files/cf_nothere");
    assert_eq!(fetch(&base, "b", &none, None).await.status(), 404);
    // The id under another room is not found there.
    let (st, g) = put(&base, "a", "dm_ab", "private.png", png()).await;
    assert_eq!(st, 200, "{g}");
    let dm = g["url"].as_str().unwrap().to_owned();
    call(&base, "a", "POST", "/api/chat/rooms/dm_ab/messages", Some(json!({ "text": "", "files": [g["id"]] }))).await;
    assert_eq!(fetch(&base, "b", &dm, None).await.status(), 200);
    assert_eq!(fetch(&base, "x", &dm, None).await.status(), 404);
    assert_eq!(fetch(&base, "c", &dm, None).await.status(), 404);
    let crossed = format!("{ROOM}/files/{}", g["id"].as_str().unwrap());
    assert_eq!(fetch(&base, "a", &crossed, None).await.status(), 404, "a file is fetched under its own room only");
    let (st, _) = put(&base, "c", "ch_general", "x.png", png()).await;
    assert_eq!(st, 404, "and uploaded only to a room you see");
}

/// SVG and HTML are text to the sniffer, and are saved, never shown;
/// a name claiming a picture changes nothing.
#[tokio::test]
async fn svg_and_html_are_never_served_inline() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' onload='alert(1)'></svg>".to_vec();
    let html = b"<!doctype html><script>alert(1)</script>".to_vec();
    let mut posted = vec![];
    for (name, body) in [("logo.svg", svg), ("page.html", html.clone()), ("fake.png", html)] {
        let (st, f) = put(&base, "a", "ch_general", name, body).await;
        assert_eq!(st, 200, "{f}");
        assert_eq!((f["kind"].clone(), f["mime"].clone()), (json!("file"), json!("text/plain")), "{name}");
        posted.push(f);
    }
    let (st, _) = say_with(&base, "a", "files", &posted.iter().collect::<Vec<_>>()).await;
    assert_eq!(st, 200);
    for f in &posted {
        let r = fetch(&base, "b", f["url"].as_str().unwrap(), None).await;
        let h = r.headers();
        assert!(h["content-disposition"].to_str().unwrap().starts_with("attachment"), "{f}");
        assert_eq!(h["content-type"], "text/plain");
        assert_eq!(h["x-content-type-options"], "nosniff");
    }
    // A picture is still saved on request.
    let (_, p) = put(&base, "a", "ch_general", "pic.png", png()).await;
    say_with(&base, "a", "", &[&p]).await;
    let r = fetch(&base, "b", &format!("{}?download=1", p["url"].as_str().unwrap()), None).await;
    assert!(r.headers()["content-disposition"].to_str().unwrap().starts_with("attachment"));
}

/// A name is kept plain, and sent as a header both old and new clients read.
#[tokio::test]
async fn names_are_cleaned_and_sent_safely() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let (_, f) = put(&base, "a", "ch_general", "../../etc/Résumé \"final\".pdf", b"%PDF-1.4\n".to_vec()).await;
    assert_eq!((f["name"].clone(), f["mime"].clone()), (json!("Résumé final.pdf"), json!("application/pdf")));
    say_with(&base, "a", "cv", &[&f]).await;
    let r = fetch(&base, "b", f["url"].as_str().unwrap(), None).await;
    let d = r.headers()["content-disposition"].to_str().unwrap().to_owned();
    assert_eq!(d, "attachment; filename=\"R_sum_ final.pdf\"; filename*=UTF-8''R%C3%A9sum%C3%A9%20final.pdf");
}

/// Video seeks by range; the answer says which bytes it is.
#[tokio::test]
async fn video_answers_ranges() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let mp4 = [b"\0\0\0\x18ftypmp42\0\0\0\0".as_slice(), &[7u8; 1000]].concat();
    let (_, f) = put(&base, "a", "ch_general", "clip.mp4", mp4.clone()).await;
    assert_eq!(f["kind"], "video");
    say_with(&base, "a", "", &[&f]).await;
    let r = fetch(&base, "b", f["url"].as_str().unwrap(), Some("bytes=100-199")).await;
    assert_eq!(r.status(), 206);
    assert_eq!(r.headers()["content-range"], format!("bytes 100-199/{}", mp4.len()).as_str());
    assert_eq!(r.bytes().await.unwrap().to_vec(), mp4[100..200]);
}

/// A file too big, an empty one, too many in a message, someone else's,
/// one used twice: each is refused. The team's total is capped too.
#[tokio::test]
async fn limits_and_ownership() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let (st, _) = put(&base, "a", "ch_general", "huge.bin", vec![1; MAX_FILE_BYTES + 1]).await;
    assert_eq!(st, 413);
    let (st, _) = put(&base, "a", "ch_general", "empty", vec![]).await;
    assert_eq!(st, 400);
    let (st, big) = put(&base, "a", "ch_general", "ok.bin", vec![1; MAX_FILE_BYTES]).await;
    assert_eq!(st, 200, "the limit itself is fine");

    // Someone else's upload isn't yours to post; one is posted once.
    let (st, p) = say_with(&base, "b", "mine now", &[&big]).await;
    assert_eq!(st, 400, "{p}");
    let (st, _) = say_with(&base, "a", "one", &[&big]).await;
    assert_eq!(st, 200);
    let (st, _) = say_with(&base, "a", "two", &[&big]).await;
    assert_eq!(st, 400, "used already");
    // Nor into another room.
    let (_, g) = put(&base, "a", "ch_general", "g.bin", vec![2; 10]).await;
    let (st, _) =
        call(&base, "a", "POST", "/api/chat/rooms/dm_ab/messages", Some(json!({ "text": "x", "files": [g["id"]] })))
            .await;
    assert_eq!(st, 400);
    // A message carries ten.
    let mut many = vec![];
    for i in 0..11 {
        let (st, f) = put(&base, "a", "ch_general", &format!("f{i}"), vec![3; 10]).await;
        assert_eq!(st, 200);
        many.push(f);
    }
    let (st, _) = say_with(&base, "a", "eleven", &many.iter().collect::<Vec<_>>()).await;
    assert_eq!(st, 400);
    let (st, p) = say_with(&base, "a", "ten", &many[..10].iter().collect::<Vec<_>>()).await;
    assert_eq!(st, 200, "{p}");
    let (_, v) = call(&base, "b", "GET", &format!("{ROOM}/messages"), None).await;
    let last = v["messages"].as_array().unwrap().last().unwrap().clone();
    let names: Vec<&str> = last["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, (0..10).map(|i| format!("f{i}")).collect::<Vec<_>>(), "in the order chosen");

    // The team's total: the database says no past the cap.
    let f = crate::db::chat_files::NewChatFile {
        id: "cf_cap",
        room: "ch_general",
        account: "a",
        scope: "team:t",
        name: "n",
        mime: "application/octet-stream",
        kind: "file",
        width: None,
        height: None,
        at: 1,
        data: &[0; 100],
    };
    let used = app.db.chat_file_bytes("team:t").unwrap();
    assert!(used >= MAX_FILE_BYTES as u64);
    assert!(!app.db.add_chat_file(&f, used + 99).unwrap());
    assert!(app.db.add_chat_file(&f, used + 100).unwrap());
    assert!(!app.db.add_chat_file(&crate::db::chat_files::NewChatFile { id: "cf_cap2", ..f }, used + 100).unwrap());
    assert_eq!(app.db.chat_file_bytes("team:other").unwrap(), 0, "a team's total is its own");
}

/// Deleting a message deletes its files; the room's other files stay.
#[tokio::test]
async fn deleting_a_message_deletes_its_files() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let (_, f) = put(&base, "b", "ch_general", "a.png", png()).await;
    let (_, keep) = put(&base, "b", "ch_general", "keep.png", png()).await;
    let (_, p) = say_with(&base, "b", "oops", &[&f]).await;
    say_with(&base, "b", "stays", &[&keep]).await;
    let url = f["url"].as_str().unwrap();
    assert_eq!(fetch(&base, "a", url, None).await.status(), 200);
    let (st, _) = call(&base, "b", "DELETE", &format!("{ROOM}/messages/{}", p["seq"]), None).await;
    assert_eq!(st, 200);
    assert_eq!(fetch(&base, "a", url, None).await.status(), 404);
    assert_eq!(fetch(&base, "b", url, None).await.status(), 404, "not even its uploader");
    assert!(app.db.chat_file(f["id"].as_str().unwrap()).unwrap().is_none(), "gone from the table, not hidden");
    assert_eq!(fetch(&base, "a", keep["url"].as_str().unwrap(), None).await.status(), 200);
    let (_, v) = call(&base, "a", "GET", &format!("{ROOM}/messages"), None).await;
    assert_eq!(v["messages"][0]["files"], json!([]));
}

/// Uploads nobody posted are swept after an hour, and not before.
#[tokio::test]
async fn unposted_uploads_are_swept() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let (_, f) = put(&base, "a", "ch_general", "left.png", png()).await;
    let id = f["id"].as_str().unwrap();
    let at = app.db.chat_file(id).unwrap().unwrap().at;
    assert_eq!(app.db.sweep_chat_files(at + 59 * 60 * 1000).unwrap(), 0);
    assert_eq!(app.db.sweep_chat_files(at + 61 * 60 * 1000).unwrap(), 1);
    assert_eq!(fetch(&base, "a", f["url"].as_str().unwrap(), None).await.status(), 404);
}

/// Search finds a message by a file's name, in rooms the searcher sees.
#[tokio::test]
async fn search_covers_file_names() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let (_, f) = put(&base, "a", "ch_general", "Quarterly-Report.pdf", b"%PDF-1.4".to_vec()).await;
    say_with(&base, "a", "", &[&f]).await;
    let (_, g) = put(&base, "a", "dm_ab", "report-secret.pdf", b"%PDF-1.4".to_vec()).await;
    call(&base, "a", "POST", "/api/chat/rooms/dm_ab/messages", Some(json!({ "text": "", "files": [g["id"]] }))).await;
    let (st, v) = call(&base, "b", "GET", "/api/chat/search?q=quarterly", None).await;
    assert_eq!(st, 200);
    assert_eq!(v["found"][0]["message"]["files"][0]["name"], "Quarterly-Report.pdf", "{v}");
    let (_, v) = call(&base, "b", "GET", "/api/chat/search?q=report", None).await;
    assert_eq!(v["found"].as_array().unwrap().len(), 2, "b is in both rooms: {v}");
    let (_, v) = call(&base, "x", "GET", "/api/chat/search?q=secret", None).await;
    assert_eq!(v["found"], json!([]), "x is not in that DM");
}

async fn geek(
    base: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    name: Option<&str>,
) -> (u16, Vec<u8>, Value) {
    let q = name.map(|n| format!("?name={}", enc(n))).unwrap_or_default();
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}{q}"))
        .header("x-test-daemon", "a geek");
    if let Some(b) = body {
        req = req.body(b);
    }
    let r = req.send().await.unwrap();
    let st = r.status().as_u16();
    let bytes = r.bytes().await.unwrap().to_vec();
    let v = serde_json::from_slice(&bytes).unwrap_or_default();
    (st, bytes, v)
}

/// An agent attaches a file with `threads.post`, and reads and fetches
/// others' with `threads.read`, at its machine's address.
#[tokio::test]
async fn an_agent_attaches_and_reads_files_within_its_grants() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let up = arugula_control_wire::chat::agent_files("ch_general");
    let (st, _, v) = geek(&base, "POST", &up, Some(png()), Some("agent.png")).await;
    assert_eq!(st, 403, "no grant: {v}");
    let scope = json!({ "kind": "project", "id": t });
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, scope.clone())).await;
    let (st, _, _) = geek(&base, "POST", &up, Some(png()), Some("agent.png")).await;
    assert_eq!(st, 403, "reading isn't posting");
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_POST, scope)).await;
    let (st, _, f) = geek(&base, "POST", &up, Some(png()), Some("agent.png")).await;
    assert_eq!(st, 200, "{f}");
    assert!(f["url"].as_str().unwrap().starts_with("/api/daemon/chat/rooms/ch_general/files/"));
    let r = reqwest::Client::new()
        .post(format!("{base}{}", arugula_control_wire::chat::agent_messages("ch_general")))
        .header("x-test-daemon", "a geek")
        .json(&json!({ "text": "here", "files": [f["id"]] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // A person sees it at the person's address; the agent at its own. (The
    // grants' system lines, #122, come before it.)
    let here = |v: &Value| v["messages"].as_array().unwrap().iter().find(|m| m["text"] == "here").cloned().unwrap();
    let (_, v) = call(&base, "b", "GET", &format!("{ROOM}/messages"), None).await;
    let m = &here(&v);
    assert_eq!(m["agent"]["machine"], "geek");
    assert!(m["files"][0]["url"].as_str().unwrap().starts_with("/api/chat/rooms/"));
    let (_, _, v) = geek(&base, "GET", &arugula_control_wire::chat::agent_messages("ch_general"), None, None).await;
    let url = here(&v)["files"][0]["url"].as_str().unwrap().to_owned();
    assert!(url.starts_with("/api/daemon/chat/rooms/ch_general/files/"), "{v}");
    let (st, bytes, _) = geek(&base, "GET", &url, None, None).await;
    assert_eq!((st, bytes), (200, png()));
}
