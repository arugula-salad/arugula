//! The desktop app's devices have a kind of their own (`app`, `Kind::App`),
//! which daemons and CLIs from before it can't read: one `app`
//! certificate in a list fails a daemon's whole answer (its trust, its
//! team's, a join's), and a CLI reads the list as empty and thinks it was
//! removed. So control leaves `app` certificates out of what it answers a
//! request that doesn't say it knows them: `x-arugula-features` naming
//! `app-devices` (this version's daemons and CLIs send it), or a cookie
//! (control's own page, which is always this version's). An older daemon
//! doesn't let an app device in until it's updated; nothing else changes
//! for it.
//!
//! Only JSON answers are looked at, and only those that hold an `app`
//! certificate are read and written again: certificate lists (whatever
//! they're called) lose their `app` ones. A certificate on its own isn't
//! touched: those go only to the device itself, which is new.

use axum::{
    body::{Body, HttpBody},
    extract::Request,
    http::{HeaderMap, header},
    middleware::Next,
    response::Response,
};
use serde_json::Value;

use arugula_control_wire::{APP_DEVICES, FEATURES_HEADER as FEATURES};

/// The most of an answer read to look for `app` certificates.
const MAX: u64 = 8 << 20;

/// Whether the request's client reads `app` certificates.
fn knows(headers: &HeaderMap) -> bool {
    let said = headers
        .get_all(FEATURES)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|f| f.trim() == APP_DEVICES);
    said || headers.contains_key(header::COOKIE)
}

/// An `app` certificate, by its shape.
fn app_cert(v: &Value) -> bool {
    v.get("kind").and_then(Value::as_str) == Some("app")
        && ["device", "sign", "noise", "approver", "account"].iter().all(|k| v.get(k).is_some())
}

/// Takes `app` certificates out of every list in `v`. Whether it took any.
fn drop_app(v: &mut Value) -> bool {
    match v {
        Value::Array(items) => {
            let before = items.len();
            items.retain(|i| !app_cert(i));
            let mut took = items.len() != before;
            for i in items {
                took |= drop_app(i);
            }
            took
        }
        Value::Object(m) => m.values_mut().fold(false, |took, i| drop_app(i) | took),
        _ => false,
    }
}

/// Leaves `app` certificates out of a JSON answer for a client that
/// doesn't know them.
pub async fn strip(req: Request, next: Next) -> Response {
    let knows = knows(req.headers());
    let res = next.run(req).await;
    let json = res.headers().get(header::CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"application/json"));
    if knows || !json || res.body().size_hint().upper().is_none_or(|n| n > MAX) {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX as usize).await else {
        return Response::from_parts(parts, Body::empty());
    };
    if !bytes.windows(12).any(|w| w == br#""kind":"app""#) {
        return Response::from_parts(parts, Body::from(bytes));
    }
    let mut v: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Response::from_parts(parts, Body::from(bytes)),
    };
    if !drop_app(&mut v) {
        return Response::from_parts(parts, Body::from(bytes));
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(serde_json::to_vec(&v).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{Cert, DeviceKeys, Kind, now_ms};
    use serde_json::json;

    use super::*;
    use crate::routing_wire::{control, daemon, person, v2};

    #[test]
    fn app_certificates_leave_lists_and_nothing_else() {
        let k = DeviceKeys::generate();
        let app = serde_json::to_value(Cert::new(&k, "a", Kind::App, "Arugula app")).unwrap();
        let browser = serde_json::to_value(Cert::new(&k, "a", Kind::Browser, "Chrome")).unwrap();
        let mut v = json!({
            "certs": [browser, app],
            "team": { "a": [[app, browser], []] },
            "block": { "kind": "app", "name": "studio" },
            "blocks": [{ "kind": "app", "name": "studio" }],
            "cert": app,
        });
        assert!(drop_app(&mut v));
        assert_eq!(v["certs"], json!([browser]));
        assert_eq!(v["team"]["a"], json!([[browser], []]));
        assert_eq!(v["blocks"], json!([{ "kind": "app", "name": "studio" }]), "a block of kind app isn't a device");
        assert_eq!(v["cert"], app, "one on its own stays");
        assert!(!drop_app(&mut v), "nothing more to take");
    }

    /// An older daemon fetching its account's certificates gets none of
    /// the app's, so it can still read the list; one that knows them, and
    /// the page, get them all.
    #[tokio::test]
    async fn older_daemons_get_lists_they_can_read() {
        let c = control(|_| {}).await;
        let root = person(&c.app, "jake", "a1");
        let d = daemon(&c.app, "a1", "geek");
        let mut app = Cert::new(&DeviceKeys::generate(), "a1", Kind::App, "Arugula app on geek");
        app.sign_with(&root);
        c.app.db.put_device(&app, true, now_ms()).unwrap();
        let kinds = |v: &Value| -> Vec<String> {
            let mut k: Vec<String> =
                v["certs"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap().to_owned()).collect();
            k.sort();
            k
        };
        let pq = "/api/daemon/trust?features=presigned-invites";
        let get = |knows: bool| {
            let mut r = c.http.get(format!("{}{pq}", c.base)).header("x-arugula-auth", v2(&d, "GET", pq, b""));
            if knows {
                r = r.header(FEATURES, format!("presigned-invites,{APP_DEVICES}"));
            }
            async move { r.send().await.unwrap().json::<Value>().await.unwrap() }
        };
        let old = get(false).await;
        assert_eq!(kinds(&old), ["browser", "daemon"]);
        // What an older daemon parses it into.
        let _: Vec<Cert> = serde_json::from_value(old["certs"].clone()).unwrap();
        assert_eq!(kinds(&get(true).await), ["app", "browser", "daemon"]);
        let cookie = crate::routing_wire::session(&c.app, "a1");
        let (_, page) = c.as_person(&cookie, "GET", "/api/devices", None).await;
        assert_eq!(kinds(&page), ["app", "browser", "daemon"]);
    }
}
