//! A hosted machine's runs (M212): the hub's routes for them, which take
//! only a hosted machine's signature (`auth::HostedAuth`), so a person's
//! machine never gets work, nor a URL to upload a run's log to (M213). The
//! runs are the platform's (`arugula_platform::work`, `run_logs`): the hub
//! says which machine asked, from the hosted certificate it checked, with
//! the key its reports are signed with, and passes the call on
//! (`PlatformPort::hosted_work`, `hosted_report`, `hosted_upload`,
//! `hosted_uploaded`), their agent sessions' reports (M257,
//! `hosted_session_report`) and push tokens (M259, `hosted_session_push`).

use std::sync::Arc;

use arugula_control_wire::{
    run_logs::{Upload, UploadGiven, Uploaded},
    work::{
        ArtifactGiven, PushAsk, PushGiven, Report, Reported, SecretsAsk, SecretsGiven, SessionIssuesAsk,
        SessionIssuesGiven, SessionReport, Work,
    },
};
use arugula_platform::{PlatformPort as _, work::HostedMachine};
use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;

use crate::{ApiError, App, auth::HostedAuth, db::hosted::HostedRow};

fn machine(h: &HostedRow) -> HostedMachine {
    HostedMachine {
        project: h.cert.project.clone(),
        workspace: h.cert.workspace.clone(),
        machine: h.cert.device.clone(),
        sign: h.cert.sign.clone(),
    }
}

#[derive(Deserialize)]
pub struct Takes {
    #[serde(default)]
    takes: String,
}

/// `GET /api/hosted/work?takes=run`: the runs ready for this machine.
pub async fn work(State(app): State<Arc<App>>, h: HostedAuth, Query(q): Query<Takes>) -> Result<Json<Work>, ApiError> {
    let takes: Vec<String> = q.takes.split(',').filter(|t| !t.is_empty()).map(str::to_owned).collect();
    Ok(Json(app.platform.hosted_work(&machine(&h.0), &takes).await?))
}

/// `POST /api/hosted/work/report`: a state change of one of its runs,
/// signed by it.
pub async fn report(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(r): Json<Report>,
) -> Result<Json<Reported>, ApiError> {
    Ok(Json(app.platform.hosted_report(&machine(&h.0), &r).await?))
}

/// `POST /api/hosted/work/session` (M257): something it says of one of
/// its agent sessions, signed by it.
pub async fn session_report(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(r): Json<SessionReport>,
) -> Result<Json<Reported>, ApiError> {
    Ok(Json(app.platform.hosted_session_report(&machine(&h.0), &r).await?))
}

/// `POST /api/hosted/work/session/push` (M259): a token to push one of its
/// sessions' branch, minted for that push. The platform mints it only for
/// this machine's own session and that session's branch.
pub async fn session_push(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(a): Json<PushAsk>,
) -> Result<Json<PushGiven>, ApiError> {
    Ok(Json(app.platform.hosted_session_push(&machine(&h.0), &a).await?))
}

/// `POST /api/hosted/work/session/issues`: a session's agent reading and
/// commenting on its own project's native issues. The platform checks the
/// session is this machine's, and answers as the session's asker.
pub async fn session_issues(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(a): Json<SessionIssuesAsk>,
) -> Result<Json<SessionIssuesGiven>, ApiError> {
    Ok(Json(app.platform.hosted_session_issues(&machine(&h.0), &a).await?))
}

/// `POST /api/hosted/work/upload`: a URL to put one of its run's log
/// chunks or artifacts at (M213).
pub async fn upload(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(u): Json<Upload>,
) -> Result<Json<UploadGiven>, ApiError> {
    Ok(Json(app.platform.hosted_upload(&machine(&h.0), &u).await?))
}

/// `POST /api/hosted/work/uploaded`: it put them; the platform records them.
pub async fn uploaded(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(u): Json<Upload>,
) -> Result<Json<Uploaded>, ApiError> {
    Ok(Json(app.platform.hosted_uploaded(&machine(&h.0), &u).await?))
}

/// `POST /api/hosted/work/secrets` (M219): a deploy run's secrets, once,
/// while it runs. Only this hosted machine's signature opens it, and the
/// platform gives them only for this machine's own run.
pub async fn secrets(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(a): Json<SecretsAsk>,
) -> Result<Json<SecretsGiven>, ApiError> {
    Ok(Json(app.platform.hosted_secrets(&machine(&h.0), &a).await?))
}

/// `POST /api/hosted/work/artifact` (M233): the artifact a deploy run of
/// this machine's applies (planter's Ship), while it runs. Only this hosted
/// machine's signature opens it, for its own run.
pub async fn artifact(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(a): Json<SecretsAsk>,
) -> Result<Json<ArtifactGiven>, ApiError> {
    Ok(Json(app.platform.hosted_artifact(&machine(&h.0), &a).await?))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use arugula_control_wire::hosted::{EnrollAsk, EnrollGiven};
    use arugula_e2e::{DeviceKeys, hosted::Delegation, now_ms, run::State};
    use arugula_platform::{
        HubPort as _,
        db::runs::{NewRun, TriggerKind},
        jobs::Worker,
        work::Dispatch,
    };
    use serde_json::{Value, json};

    use crate::{
        auth::hash,
        routing_wire::{Control, control, daemon, person, session, v2},
    };

    impl Control {
        async fn signed(&self, keys: &DeviceKeys, method: &str, pq: &str, body: Option<&Value>) -> (u16, Value) {
            let bytes = body.map(|b| serde_json::to_vec(b).unwrap()).unwrap_or_default();
            let mut r = self
                .http
                .request(method.parse().unwrap(), format!("{}{pq}", self.base))
                .header("x-arugula-auth", v2(keys, method, pq, &bytes));
            if body.is_some() {
                r = r.header("content-type", "application/json").body(bytes);
            }
            let r = r.send().await.unwrap();
            (r.status().as_u16(), r.json().await.unwrap_or_default())
        }
    }

    /// A hosted machine pulls the run the platform has for it, through the
    /// hub with its own signature, and reports it running, signed; the
    /// platform keeps the move. A person's machine is refused both routes.
    #[tokio::test]
    async fn a_hosted_machine_pulls_its_run_and_reports_it() {
        let c = control(|_| {}).await;
        let me = person(&c.app, "me", "m1");
        let me_s = session(&c.app, "m1");
        let p = c.app.platform.inner().clone();
        let project = p.db.personal_project("m1", now_ms()).unwrap();
        let mut d = Delegation {
            project: project.clone(),
            owner: "account:m1".into(),
            key: c.app.hosted_key.public(),
            at: now_ms(),
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&me);
        let (st, _) = c.as_person(&me_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
        assert_eq!(st, 200);
        let ask = EnrollAsk {
            project: project.clone(),
            owner: "account:m1".into(),
            workspace: "hw_7".into(),
            name: "box".into(),
            sandbox: None,
            purpose: None,
        };
        let EnrollGiven::Ready { ticket: Some(t), .. } = c.app.hub_port.hosted_enrollment(&ask).await.unwrap() else {
            panic!("no ticket")
        };
        let k = DeviceKeys::generate();
        let (noise, sign) = (hex::encode(k.noise_public), hex::encode(k.sign_public()));
        let at = now_ms();
        let proof =
            hex::encode(k.signature(arugula_e2e::hosted::enroll_proof_body(&hash(&t), &noise, &sign, at).as_bytes()));
        let body = json!({ "ticket": t, "noise": noise, "sign": sign, "at": at, "proof": proof });
        let r = c.http.post(format!("{}/api/hosted/enroll", c.base)).json(&body).send().await.unwrap();
        assert_eq!(r.status(), 200);

        // The platform's workspace, whose box enrolled as this machine, and
        // a run on it under a standing grant.
        let made = p.db.add_hosted_workspace("hw_7", &project, "w", &Default::default(), "m1", 10, 1).unwrap();
        assert!(matches!(made, arugula_platform::db::hosted::Added::Made(_)));
        // As the platform's worker marks it once the hub says it enrolled.
        let mut job = p.db.claim_hosted(now_ms()).unwrap().expect("the new workspace is due");
        job.w.state = "ready".into();
        job.w.machine = Some(k.id());
        p.db.checkpoint_hosted(&job, None, now_ms() + 3_600_000, now_ms()).unwrap();
        let op = format!("arugula op v1\ndaemon {}\nkind run\nsession -\npane - -\ncwd -\nargv dHJ1ZQ==\n", k.id());
        let new = NewRun {
            project: &project,
            workspace: Some("hw_7"),
            trigger: TriggerKind::Manual,
            source: &json!({}),
            op: &op,
            started_by: "m1",
            started_by_agent: None,
            grant: Some("g1"),
        };
        let run = p.db.create_run(&new, now_ms()).unwrap().unwrap().id;
        arugula_platform::db::runs::dispatch(&p.db.c(), &run, now_ms()).unwrap();
        let worker = Worker::new().with(Dispatch { app: p.clone() });
        let pq = "/api/hosted/work?takes=run";
        let mut got = Value::Null;
        for _ in 0..50 {
            let _ = worker.tick(&p.db).await;
            let (st, v) = c.signed(&k, "GET", pq, None).await;
            assert_eq!(st, 200, "{v}");
            if v["items"].as_array().is_some_and(|i| !i.is_empty()) {
                got = v;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(got["items"][0]["run"], json!(run), "{got}");
        assert_eq!(got["items"][0]["op"], json!(op));

        // Its report, signed, moves the run.
        let ev = arugula_e2e::run::Event {
            run: &run,
            seq: 2,
            from: Some(State::Pending),
            to: State::Running,
            at: now_ms(),
            detail: "{}",
            daemon: &k.id(),
        };
        let report = json!({ "run": run, "seq": 2, "from": "pending", "to": "running", "at": ev.at,
                             "detail": "{}", "sig": ev.sign(&k) });
        let (st, v) = c.signed(&k, "POST", "/api/hosted/work/report", Some(&report)).await;
        assert_eq!((st, v), (200, json!({ "kind": "taken" })));
        assert_eq!(p.db.run(&run).unwrap().unwrap().state, State::Running);
        let (_, v) = c.signed(&k, "GET", pq, None).await;
        assert_eq!(v, json!({ "items": [], "revokes": [] }));

        // Its log (M213): this control has no run-log bucket, so it's off.
        let up = json!({ "kind": "log", "run": run, "n": 0, "offset": 0, "size": 1, "sha256": "0".repeat(64) });
        let (st, v) = c.signed(&k, "POST", "/api/hosted/work/upload", Some(&up)).await;
        assert_eq!((st, v), (200, json!({ "kind": "off" })));

        // A person's machine: none of the routes.
        let mine = daemon(&c.app, "m1", "laptop");
        assert_eq!(c.signed(&mine, "GET", pq, None).await.0, 401);
        assert_eq!(c.signed(&mine, "POST", "/api/hosted/work/report", Some(&report)).await.0, 401);
        assert_eq!(c.signed(&mine, "POST", "/api/hosted/work/upload", Some(&up)).await.0, 401);
        assert_eq!(c.signed(&mine, "POST", "/api/hosted/work/uploaded", Some(&up)).await.0, 401);
        let issues = json!({ "session": "ses_x", "op": "list", "query": {} });
        assert_eq!(c.signed(&mine, "POST", "/api/hosted/work/session/issues", Some(&issues)).await.0, 401);
        // The hosted machine's own is let through to the platform, which
        // refuses a session that isn't there.
        let (st, v) = c.signed(&k, "POST", "/api/hosted/work/session/issues", Some(&issues)).await;
        assert_eq!((st, v["kind"].as_str()), (200, Some("refused")), "{v}");
    }
}
