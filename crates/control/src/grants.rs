//! Grants (M130, "Grants, not impersonation"): what a machine may do with
//! its owner's platform objects. For now that's one power, `cloud.drive`:
//! the agents on a machine drive its owner's Cloud workspaces (aru-047).
//! Every machine that joins an account gets it; the owner lists, revokes
//! and gives it back here (`arugula grants` in the CLI), and
//! `auth::CloudDriver` refuses a machine without a live one.
//!
//! Only a person (a signed-in browser, or a CLI device) manages grants: a
//! machine's signature isn't a [`Session`], so it can't give itself one.

use std::sync::Arc;

use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    ApiError, App,
    auth::Session,
    db::projects::{CLOUD_DRIVE, Grant},
    err,
};

type R = Result<Json<Value>, ApiError>;

fn wire(g: &Grant) -> Value {
    json!({
        "id": g.id,
        "subject": { "kind": g.subject_kind, "id": g.subject_id, "name": g.subject_name },
        "power": g.power,
        "scope": { "kind": g.scope_kind, "id": g.scope_id },
        "granted_by": g.granted_by,
        "created_at": g.created_at,
        "revoked_at": g.revoked_at,
        "live": g.revoked_at.is_none(),
    })
}

/// `GET /api/grants`: the grants over the signed-in account, live and
/// revoked, by machine.
pub async fn list(State(app): State<Arc<App>>, s: Session) -> R {
    let grants = app.db.grants(&s.account)?;
    Ok(Json(json!({ "grants": grants.iter().map(wire).collect::<Vec<_>>() })))
}

/// `POST /api/grants/{id}/revoke`: take a grant back. The machine is
/// refused from its next call; revoking again changes nothing.
pub async fn revoke(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>) -> R {
    let g =
        app.db.revoke_grant(&s.account, &id, now_ms())?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such grant"))?;
    Ok(Json(wire(&g)))
}

#[derive(Deserialize)]
pub struct Give {
    /// The machine (its id).
    daemon: String,
    power: String,
}

/// `POST /api/grants {daemon, power}`: give one of the account's machines
/// `cloud.drive` again, as a new grant. One it holds already is the answer.
pub async fn give(State(app): State<Arc<App>>, s: Session, Json(b): Json<Give>) -> R {
    if b.power != CLOUD_DRIVE {
        return Err(err(StatusCode::BAD_REQUEST, "the one power there is to give is cloud.drive"));
    }
    let g = app
        .db
        .give_cloud_drive(&s.account, &b.daemon, &s.account, now_ms())?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "not a machine on this account"))?;
    Ok(Json(wire(&g)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing_wire::{daemon, v2};

    /// Revoking one machine's grant refuses that machine on the Cloud
    /// workspace routes and leaves the owner's other machine alone; giving
    /// it back lets it in again.
    #[tokio::test]
    async fn a_revoked_grant_refuses_only_its_own_machine() {
        let app = crate::cloud::tests::test_app();
        let (geek, mini) = (daemon(&app, "a", "geek"), daemon(&app, "a", "mini"));
        let (geek_id, mini_id) = (geek.id(), mini.id());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, crate::router(app)).await.unwrap() });
        let client = reqwest::Client::new();
        let call = |keys: &arugula_e2e::DeviceKeys, method: &str, path: &str, body: Option<Value>| {
            let bytes = body.map(|b| serde_json::to_vec(&b).unwrap()).unwrap_or_default();
            let mut req = client
                .request(method.parse().unwrap(), format!("{base}{path}"))
                .header("x-arugula-auth", v2(keys, method, path, &bytes));
            if !bytes.is_empty() {
                req = req.header("content-type", "application/json").body(bytes);
            }
            async move {
                let r = req.send().await.unwrap();
                (r.status(), r.json::<Value>().await.unwrap_or_default())
            }
        };
        let person = |method: &str, path: &str, body: Option<Value>| {
            let mut req = client
                .request(method.parse().unwrap(), format!("{base}{path}"))
                .header("cookie", "ilg_session=session-a");
            if let Some(b) = body {
                req = req.json(&b);
            }
            async move {
                let r = req.send().await.unwrap();
                (r.status(), r.json::<Value>().await.unwrap_or_default())
            }
        };

        // Both joined machines hold cloud.drive over the account.
        let (status, listed) = person("GET", "/api/grants", None).await;
        assert_eq!(status, StatusCode::OK);
        let grants = listed["grants"].as_array().unwrap();
        assert_eq!(grants.len(), 2, "{listed}");
        assert!(grants.iter().all(|g| g["power"] == CLOUD_DRIVE && g["live"] == true && g["scope"]["id"] == "a"));
        assert_eq!(grants[0]["subject"]["name"], "geek");
        let geeks = grants[0]["id"].as_str().unwrap().to_owned();
        let (status, made) =
            call(&geek, "POST", "/api/cloud/workspaces", Some(json!({"request_key":"k","spec":{"name":"box"}}))).await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let id = made["id"].as_str().unwrap().to_owned();

        // A machine can't manage grants, not even its own.
        assert_eq!(call(&geek, "GET", "/api/grants", None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(call(&geek, "POST", &format!("/api/grants/{geeks}/revoke"), None).await.0, StatusCode::UNAUTHORIZED);

        let (status, revoked) = person("POST", &format!("/api/grants/{geeks}/revoke"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(revoked["live"], false);
        // What `cloud` and `read_cloud` call, from the revoked machine:
        // refused, naming it and how to restore it.
        let tab =
            json!({"request_key":"t","agent_id":"curated_coding","runtime":"claude","model":"anthropic/test-model"});
        for (method, path, body) in [
            ("GET", "/api/cloud/workspaces".to_owned(), None),
            ("POST", "/api/cloud/workspaces".to_owned(), Some(json!({"request_key":"k2","spec":{"name":"x"}}))),
            ("GET", format!("/api/cloud/workspaces/{id}"), None),
            ("GET", format!("/api/cloud/workspaces/{id}/tabs"), None),
            ("POST", format!("/api/cloud/workspaces/{id}/tabs"), Some(tab.clone())),
            ("GET", format!("/api/cloud/workspaces/{id}/files"), None),
            ("POST", format!("/api/cloud/workspaces/{id}/retry"), None),
            ("GET", "/api/cloud/models".to_owned(), None),
            ("GET", "/api/cloud/agents".to_owned(), None),
        ] {
            let (status, said) = call(&geek, method, &path, body).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {said}");
            let said = said["error"].as_str().unwrap_or_default();
            assert!(said.starts_with("geek can't drive") && said.contains("arugula grants restore geek"), "{said}");
        }
        // The owner's other machine, and the owner, still drive it.
        let (status, listed) = call(&mini, "GET", "/api/cloud/workspaces", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["cloud_workspaces"][0]["id"], id.as_str());
        let (status, opened) = call(&mini, "POST", &format!("/api/cloud/workspaces/{id}/tabs"), Some(tab)).await;
        assert_eq!(status, StatusCode::OK, "{opened}");
        assert_eq!(person("GET", "/api/cloud/workspaces", None).await.0, StatusCode::OK);

        // Another account can't give it back, or give one for a machine
        // that isn't its own; the owner can.
        let give = json!({"daemon": geek_id, "power": CLOUD_DRIVE});
        let (status, _) = person("POST", "/api/grants", Some(json!({"daemon": "nope", "power": CLOUD_DRIVE}))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = person("POST", "/api/grants", Some(json!({"daemon": geek_id, "power": "cloud.admin"}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let other = client
            .post(format!("{base}/api/grants"))
            .header("cookie", "ilg_session=session-b")
            .json(&give)
            .send()
            .await
            .unwrap();
        assert_eq!(other.status(), StatusCode::NOT_FOUND);
        let (status, given) = person("POST", "/api/grants", Some(give.clone())).await;
        assert_eq!(status, StatusCode::OK);
        assert!(given["live"] == true && given["id"] != geeks.as_str(), "a new grant: {given}");
        assert_eq!(person("POST", "/api/grants", Some(give)).await.1["id"], given["id"], "a live one is the answer");
        assert_eq!(call(&geek, "GET", "/api/cloud/workspaces", None).await.0, StatusCode::OK);
        // The revoked one stays on the list.
        let (_, listed) = person("GET", "/api/grants", None).await;
        let states: Vec<_> = listed["grants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| (g["subject"]["id"].clone(), g["live"].clone()))
            .collect();
        assert_eq!(
            states,
            [(json!(geek_id), json!(true)), (json!(geek_id), json!(false)), (json!(mini_id), json!(true))]
        );
        server.abort();
    }
}
