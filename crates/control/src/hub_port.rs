//! The hub's side of the seam with the platform (#1153), in process:
//! [`InProcess`], the platform's `HubPort` over the hub's own state, and
//! [`gate`], which tells the platform's routes who's asking.
//!
//! The platform calls these and nothing else of the hub's. M140 serves the
//! same calls over the private network, and signs the principal into a
//! header (`arugula_control_common::principal`) where [`gate`] puts it in
//! the request's extensions. `arugula_platform::contract` checks that both
//! answer the same (the tests below run it here).

use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock, Weak},
};

use arugula_control_common::Principal;
use arugula_e2e::Kind;
use arugula_platform::{
    auth::{Asked, Credential},
    hub::{Account, ChangePage, Daemon, Flags, HubPort, HubResult, Member, Meter, Pushed, Resolved, Team},
};
use axum::{
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::Response,
};
use futures_util::future::BoxFuture;

use crate::{App, auth, teams};

/// `HubPort` over the hub's own state. It holds the app weakly: the app
/// holds the platform, which holds this. Attached once the app is in its
/// `Arc` ([`App::start`]); a call before then, or after the app is gone,
/// is an error.
#[derive(Default)]
pub struct InProcess {
    app: OnceLock<Weak<App>>,
}

impl InProcess {
    pub fn attach(&self, app: Weak<App>) {
        let _ = self.app.set(app);
    }

    fn app(&self) -> HubResult<Arc<App>> {
        self.app.get().and_then(Weak::upgrade).ok_or_else(|| anyhow::anyhow!("the hub isn't running"))
    }
}

fn account_of(app: &App, id: &str) -> HubResult<Option<Account>> {
    Ok(app.db.account(id)?.map(|a| Account { id: a.id, login: a.login, name: a.name }))
}

fn daemon_of(app: &App, id: &str) -> HubResult<Option<Daemon>> {
    let Some((account, row)) = app.db.daemon_row(id)? else { return Ok(None) };
    Ok(Some(Daemon { id: row.id, account, team: app.db.daemon_team(id)?, name: row.name }))
}

fn resolve(app: &App, p: &Principal) -> HubResult<Option<Resolved>> {
    let Some(account) = account_of(app, p.account())? else { return Ok(None) };
    let daemon = match p {
        Principal::Person { .. } => None,
        Principal::Cli { account: a, device } => {
            let approved = matches!(app.db.device(a, device)?, Some((cert, true)) if cert.kind == Kind::Cli);
            if !approved || app.db.revocations(a)?.iter().any(|r| &r.device == device) {
                return Ok(None);
            }
            None
        }
        Principal::Daemon { account: a, daemon } => match daemon_of(app, daemon)? {
            Some(d) if &d.account == a && app.db.revoked(daemon)?.is_none() => Some(d),
            _ => return Ok(None),
        },
    };
    Ok(Some(Resolved { principal: p.clone(), account, daemon }))
}

fn team_of(app: &App, id: &str) -> HubResult<Option<Team>> {
    let (Some(t), Some(body)) = (app.db.team(id)?, app.db.latest_roster(id)?) else { return Ok(None) };
    let r = teams::parse(&body)?;
    let mut members = Vec::new();
    for m in r.members {
        if app.db.account(&m.account)?.is_some() {
            members.push(Member { account: m.account, role: m.role, name: m.name });
        }
    }
    Ok(Some(Team { id: t.id, name: t.name, founder: t.founder, locked: t.locked, version: r.version, members }))
}

fn teams_of(app: &App, account: &str) -> HubResult<Vec<String>> {
    let mut ids =
        app.db.teams_of(account)?.iter().map(|b| Ok(teams::parse(b)?.team)).collect::<HubResult<Vec<String>>>()?;
    ids.sort();
    Ok(ids)
}

fn machines_for(app: &App, account: &str) -> HubResult<Vec<String>> {
    let mut ids: Vec<String> = app.db.daemons(account)?.into_iter().map(|d| d.id).collect();
    ids.extend(teams::reachable(app, account)?);
    ids.sort();
    ids.dedup();
    Ok(ids)
}

impl HubPort for InProcess {
    fn resolve<'a>(&'a self, principal: &'a Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>> {
        Box::pin(async move {
            let app = self.app()?;
            resolve(&app, principal)
        })
    }

    fn accounts<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        Box::pin(async move {
            let app = self.app()?;
            let mut out = Vec::new();
            for id in ids {
                out.extend(account_of(&app, id)?);
            }
            Ok(out)
        })
    }

    fn accounts_called<'a>(&'a self, word: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        Box::pin(async move {
            let app = self.app()?;
            if let Some(a) = app.db.account_by_login(word)? {
                return Ok(vec![Account { id: a.id, login: a.login, name: a.name }]);
            }
            let mut out = Vec::new();
            for id in app.db.accounts_named(word)? {
                out.extend(account_of(&app, &id)?);
            }
            Ok(out)
        })
    }

    fn teams_of<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        Box::pin(async move {
            let app = self.app()?;
            teams_of(&app, account)
        })
    }

    fn teams<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>> {
        Box::pin(async move {
            let app = self.app()?;
            let mut out = Vec::new();
            for id in ids {
                out.extend(team_of(&app, id)?);
            }
            Ok(out)
        })
    }

    fn daemons<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>> {
        Box::pin(async move {
            let app = self.app()?;
            let mut out = Vec::new();
            for id in ids {
                out.extend(daemon_of(&app, id)?);
            }
            Ok(out)
        })
    }

    fn machines_for<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        Box::pin(async move {
            let app = self.app()?;
            machines_for(&app, account)
        })
    }

    fn may_reach<'a>(&'a self, account: &'a str, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
        Box::pin(async move {
            let app = self.app()?;
            let mut out = Vec::new();
            for m in machines {
                if teams::may_reach(&app, account, m)? {
                    out.push(m.clone());
                }
            }
            Ok(out)
        })
    }

    fn push<'a>(&'a self, account: &'a str, message: &'a serde_json::Value) -> BoxFuture<'a, HubResult<Pushed>> {
        Box::pin(async move {
            let app = self.app()?;
            crate::push::push_account(&app, account, message.to_string().as_bytes()).await
        })
    }

    fn labs_flags<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>> {
        Box::pin(async move {
            let app = self.app()?;
            let mut out = BTreeMap::new();
            for o in owners {
                let flags = app.db.labs_flags(o)?;
                if !flags.is_empty() {
                    out.insert(o.clone(), flags);
                }
            }
            Ok(out)
        })
    }

    fn meters<'a>(&'a self, accounts: &'a [String], since: u64, until: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>> {
        Box::pin(async move {
            let app = self.app()?;
            let month = &crate::day(until)[..7];
            accounts
                .iter()
                .map(|a| {
                    Ok(Meter {
                        account: a.clone(),
                        relay_bytes: app.db.relay_bytes_month(a, month)?,
                        sandbox_minutes: app.db.sandbox_minutes(a, since, until)?,
                    })
                })
                .collect()
        })
    }

    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>> {
        Box::pin(async move { self.app()?.db.sandbox_accounts() })
    }

    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>> {
        Box::pin(async move { self.app()?.db.changes(after, limit) })
    }

    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        Box::pin(async move { self.app()?.db.personal_project(account, arugula_e2e::now_ms()) })
    }

    fn can_drive_cloud<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        Box::pin(async move { self.app()?.db.can_drive_cloud(daemon, account) })
    }
}

/// In front of the platform's routes, in process: who's asking, as the
/// hub checks it for its own routes, put in the request for the platform's
/// extractors (`arugula_platform::auth`). A daemon's or CLI device's
/// signature (already checked by `auth::verify_daemon`) or a refusal of
/// it; else the session, with `auth::Session`'s same-origin check, or its
/// refusal. M140's gateway does the same and signs the principal into a
/// header.
pub async fn gate(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    // Only the hub says who's asking.
    parts.extensions.remove::<Asked>();
    let signed = parts.extensions.get::<auth::Signed>().map(|s| s.0.clone());
    let asked = match signed {
        Some(Ok(cert)) if cert.kind == Kind::Daemon => {
            Some(Asked::Principal(Principal::Daemon { account: cert.account, daemon: cert.device }))
        }
        Some(Ok(cert)) if cert.kind == Kind::Cli => {
            // As a session: it notes the account for analytics.
            let _ = auth::Session::from_request_parts(&mut parts, &app).await;
            Some(Asked::Principal(Principal::Cli { account: cert.account, device: cert.device }))
        }
        Some(Ok(_)) => None,
        Some(Err((status, error))) => Some(Asked::Refused { status, error, by: Credential::Signature }),
        None => Some(match auth::Session::from_request_parts(&mut parts, &app).await {
            Ok(s) => Asked::Principal(Principal::Person { account: s.account }),
            Err(e) => Asked::Refused { status: e.0, error: e.1, by: Credential::Session },
        }),
    };
    if let Some(a) = asked {
        parts.extensions.insert(a);
    }
    next.run(Request::from_parts(parts, body)).await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use arugula_e2e::{
        Cert, DeviceKeys, Kind, Revocation,
        push::PushSub,
        team::{Member, Roster, TeamRole},
    };
    use arugula_platform::{HubPort, contract};

    use crate::App;

    /// The hub, seeded through its own database calls, with a push service
    /// of its own.
    struct Fixture {
        app: Arc<App>,
        push_at: String,
        pushes: Arc<AtomicUsize>,
    }

    impl Fixture {
        async fn new() -> Self {
            use axum::{Router, extract::Path, http::StatusCode, routing::post};
            let pushes = Arc::new(AtomicUsize::new(0));
            let count = pushes.clone();
            let service = Router::new().route(
                "/push/{kind}",
                post(move |Path(kind): Path<String>| {
                    let count = count.clone();
                    async move {
                        if kind == "gone" {
                            return StatusCode::GONE;
                        }
                        count.fetch_add(1, Ordering::SeqCst);
                        StatusCode::CREATED
                    }
                }),
            );
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = l.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(l, service).await });
            let mut app = App::for_tests("http://control.test");
            app.cfg.push_hosts.push(at.to_string());
            Fixture { app: app.start(), push_at: format!("http://{at}/push"), pushes }
        }
    }

    impl contract::Fixture for Fixture {
        fn hub(&self) -> Arc<dyn HubPort> {
            self.app.platform.hub.clone()
        }

        fn account(&self, id: &str, login: &str, name: &str) -> anyhow::Result<()> {
            self.app.db.account_for("github", id, login, id, arugula_e2e::now_ms())?;
            if !name.is_empty() {
                self.app.db.set_name(id, name)?;
            }
            Ok(())
        }

        fn rename(&self, account: &str, name: &str) -> anyhow::Result<()> {
            self.app.db.set_name(account, name)
        }

        fn cli(&self, account: &str) -> anyhow::Result<String> {
            let keys = DeviceKeys::generate();
            let mut cert = Cert::new(&keys, account, Kind::Cli, "a CLI");
            cert.sign_with(&keys);
            self.app.db.put_device(&cert, true, arugula_e2e::now_ms())?;
            Ok(cert.device)
        }

        fn revoke(&self, account: &str, device: &str) -> anyhow::Result<()> {
            self.app.db.add_revocation(&Revocation::new(account, device, &DeviceKeys::generate()))
        }

        fn roster(&self, team: &str, name: &str, founder: &str, members: &[(&str, TeamRole)]) -> anyhow::Result<()> {
            let version = match self.app.db.latest_roster(team)? {
                Some(body) => crate::teams::parse(&body)?.version + 1,
                None => 1,
            };
            let r = Roster {
                v: 2,
                team: team.into(),
                name: name.into(),
                version,
                at: arugula_e2e::now_ms(),
                members: members
                    .iter()
                    .map(|(a, role)| Member {
                        account: (*a).into(),
                        root: String::new(),
                        role: *role,
                        name: (*a).into(),
                    })
                    .collect(),
                spent: vec![],
                redeem: None,
                by: String::new(),
                sig: String::new(),
            };
            let body = serde_json::to_string(&r)?;
            if version == 1 {
                let t = crate::db::Team {
                    id: team.into(),
                    name: name.into(),
                    founder: founder.into(),
                    founder_root: String::new(),
                    locked: false,
                };
                self.app.db.add_team(&t, 1, &body, arugula_e2e::now_ms())
            } else {
                self.app.db.add_roster(team, version, &body)
            }
        }

        fn lock(&self, team: &str, locked: bool) -> anyhow::Result<()> {
            self.app.db.set_locked(team, locked)
        }

        fn daemon(&self, id: &str, account: &str, name: &str) -> anyhow::Result<()> {
            self.app.db.put_daemon(account, id, name, &[])
        }

        fn move_daemon(&self, id: &str, team: Option<&str>) -> anyhow::Result<()> {
            self.app.db.move_daemon(id, team, "{}")
        }

        fn share(&self, daemon: &str, account: &str) -> anyhow::Result<()> {
            self.app.db.set_access(daemon, &[account.to_owned()], None)?;
            self.app.db.answer_share(account, daemon, true, arugula_e2e::now_ms())
        }

        fn leave(&self, daemon: &str) -> anyhow::Result<()> {
            self.app.db.drop_daemon(daemon)
        }

        fn flag(&self, owner: &str, name: &str, on: Option<bool>) -> anyhow::Result<()> {
            self.app.db.set_labs_flag(owner, name, on).map(|_| ())
        }

        fn relay_bytes(&self, account: &str, ms: u64, bytes: u64) -> anyhow::Result<()> {
            self.app.db.add_relay_bytes(account, &crate::day(ms), bytes)
        }

        fn sandbox(&self, account: &str, id: &str, from: u64, to: u64) -> anyhow::Result<()> {
            self.app.db.add_sandbox(id, account, "", id, from)?;
            self.app.db.end_sandbox(id, to)
        }

        fn delete(&self, account: &str) -> anyhow::Result<()> {
            let p = crate::account::plan(&self.app, account)?;
            let disband: Vec<String> = p.disband.iter().map(|(t, _, _)| t.clone()).collect();
            self.app.db.delete_account(&crate::db::Erase { account, disband: &disband, retain_for: &p.retain_for })?;
            Ok(())
        }

        fn subscribe(&self, account: &str, gone: bool) -> anyhow::Result<()> {
            use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
            let keys = DeviceKeys::generate();
            let mut cert = Cert::new(&keys, account, Kind::Browser, "a browser");
            cert.sign_with(&keys);
            self.app.db.put_device(&cert, true, 0)?;
            let browser = p256::SecretKey::from_slice(&[1u8; 32]).unwrap();
            let mut sub = PushSub {
                v: 1,
                account: account.into(),
                device: String::new(),
                endpoint: format!("{}/{}", self.push_at, if gone { "gone" } else { "ok" }),
                p256dh: B64.encode(browser.public_key().to_sec1_bytes()),
                auth: B64.encode([2u8; 16]),
                at: 0,
                sig: String::new(),
            };
            sub.sign_with(&keys);
            self.app.db.put_push_sub(&sub.endpoint, account, &serde_json::to_string(&sub)?)
        }

        fn pushes(&self) -> usize {
            self.pushes.load(Ordering::SeqCst)
        }

        fn prune_changes(&self, now: u64) -> anyhow::Result<()> {
            self.app.db.prune_changes(now).map(|_| ())
        }

        fn machine(&self, account: &str, name: &str) -> anyhow::Result<String> {
            let keys = DeviceKeys::generate();
            let cert = Cert::new(&keys, account, Kind::Daemon, name);
            self.app.db.put_device(&cert, true, arugula_e2e::now_ms())?;
            self.app.db.put_daemon(account, &cert.device, name, &[])?;
            Ok(cert.device)
        }

        fn revoke_cloud_drive(&self, account: &str, daemon: &str) -> anyhow::Result<()> {
            for g in self.app.db.grants(account)? {
                if g.subject_id == daemon {
                    self.app.db.revoke_grant(account, &g.id, arugula_e2e::now_ms())?;
                }
            }
            Ok(())
        }
    }

    /// The platform's contract, against the in-process port.
    #[tokio::test]
    async fn in_process_keeps_the_contract() {
        contract::run(&Fixture::new().await).await;
    }

    /// The gate answers who's asking as control's own extractors do: a
    /// session (with its same-origin check), a daemon's signature, or a
    /// CLI device's; and their refusals.
    #[tokio::test]
    async fn the_gate_says_who_is_asking() {
        use arugula_platform::auth::{Machine, Person};
        use axum::routing::{get, post};
        let app = App::for_tests("http://control.test").start();
        let now = arugula_e2e::now_ms();
        app.db.account_for("github", "1", "alice", "acct-a", now).unwrap();
        app.db.add_session(&crate::auth::hash("tok"), "acct-a", now, now + 60_000).unwrap();
        let keys = DeviceKeys::generate();
        let cert = Cert::new(&keys, "acct-a", Kind::Daemon, "geek");
        app.db.put_device(&cert, true, now).unwrap();
        app.db.put_daemon("acct-a", &cert.device, "geek", &[]).unwrap();
        let routes = axum::Router::new()
            .route("/person", get(|p: Person| async move { p.account }).post(|p: Person| async move { p.account }))
            .route("/machine", post(|m: Machine| async move { format!("{} {}", m.account, m.daemon) }))
            .layer(axum::middleware::from_fn_with_state(app.clone(), super::gate))
            .layer(axum::middleware::from_fn_with_state(app.clone(), crate::auth::verify_daemon))
            .with_state(app.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, routes).await });
        let http = reqwest::Client::new();
        let cookie = format!("{}=tok", crate::auth::SESSION_COOKIE);
        let answer = |r: reqwest::Response| async move { (r.status().as_u16(), r.text().await.unwrap()) };

        let r = http.get(format!("{base}/person")).header("cookie", &cookie).send().await.unwrap();
        assert_eq!(answer(r).await, (200, "acct-a".into()));
        let r = http.get(format!("{base}/person")).send().await.unwrap();
        assert_eq!(answer(r).await, (401, r#"{"error":"sign in first"}"#.into()));
        let r = http
            .post(format!("{base}/person"))
            .header("cookie", &cookie)
            .header("origin", "https://elsewhere.test")
            .send()
            .await
            .unwrap();
        assert_eq!(answer(r).await, (403, r#"{"error":"cross-origin request refused"}"#.into()));
        let r = http.post(format!("{base}/machine")).header("cookie", &cookie).send().await.unwrap();
        assert_eq!(answer(r).await, (401, r#"{"error":"bad daemon signature"}"#.into()), "a person isn't a machine");

        let ms = arugula_e2e::now_ms();
        let nonce = hex::encode(arugula_e2e::random::<16>());
        let msg = crate::auth::daemon_auth_message_v2("POST", "/machine", ms, &nonce, b"");
        let sig = format!("v2 {} {ms} {nonce} {}", keys.id(), hex::encode(keys.signature(msg.as_bytes())));
        let r = http.post(format!("{base}/machine")).header("x-arugula-auth", &sig).send().await.unwrap();
        assert_eq!(answer(r).await, (200, format!("acct-a {}", cert.device)));
        let r = http.post(format!("{base}/machine")).header("x-arugula-auth", &sig).send().await.unwrap();
        assert_eq!(answer(r).await.0, 401, "a signature is good once, and its refusal reaches the platform");
    }

    /// An app that wasn't started has no hub to call, and says so.
    #[tokio::test]
    async fn detached_says_so() {
        let app = App::for_tests("http://control.test");
        let e = app.platform.hub.changes(0, 1).await.unwrap_err();
        assert_eq!(e.to_string(), "the hub isn't running");
    }
}
