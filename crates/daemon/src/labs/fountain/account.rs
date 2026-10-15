//! This daemon's Fountain account (#942, F1): its own sign-in, the login
//! everything Fountain uses, and the account stream's status.
//!
//! **Sign-in** is Fountain's device flow: `POST /api/auth/device` gives a
//! code to approve at `/device` in a browser signed in to Fountain (on the
//! phone, say); the daemon polls `/api/auth/device/token` every `interval`
//! seconds (five more on `slow_down`) until the code is approved, denied or
//! expired. Started by `arugula fountain login [URL]` or `POST
//! /api/fountain/login`, which any client (the web, the phone) can call.
//! Which Fountain: the URL given, else the one signed in to, else
//! `$FOUNTAIN_BASE_URL` or the CLI profile's `base_url`; with none, it asks
//! for one. There is no default instance.
//!
//! **The key** is kept in the state dir, `fountain/login.json`, mode 0600 in
//! a 0700 directory (as `studio.json` is), and held in memory: never logged,
//! never sent to a client (the status says where it came from and its
//! prefix). `arugula fountain logout` deletes the file and revokes the key on
//! Fountain when it can (nothing else holds it).
//!
//! **Which login is used** ([`Account::resolve`]): this daemon's sign-in,
//! else today's resolution ([`super::login`]: `$FOUNTAIN_API_KEY`, else the
//! `fountain` CLI's `~/.fountain/credentials`), read on this host with the
//! user's shell environment.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use arugula_proto::api::{FountainDevice, FountainLogin, FountainStream};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, warn};

use super::{
    client::{self, Client, DeviceGrant, Poll},
    login,
    stream::Hub,
};
use crate::{shellenv::ShellEnv, store::now_ms};

/// What's kept: the account's URL and key, and what may be shown of it.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Saved {
    base_url: String,
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    #[serde(default)]
    at_ms: u64,
}

/// Where the key in use comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `arugula fountain login`.
    Arugula,
    /// `$FOUNTAIN_API_KEY`.
    Env,
    /// The `fountain` CLI's credentials file.
    File,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Arugula => "arugula",
            Source::Env => "env",
            Source::File => "file",
        }
    }
}

/// A login to use: the account's URL and key, and where it came from.
#[derive(Clone, PartialEq, Eq)]
pub struct Resolved {
    pub base_url: String,
    pub key: String,
    pub source: Source,
    /// Fountain's id for the account, when known (an Arugula sign-in).
    pub user_id: Option<String>,
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved")
            .field("base_url", &self.base_url)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl Resolved {
    pub fn client(&self) -> Client {
        Client::new(&self.base_url, &self.key)
    }
}

/// A sign-in waiting for approval.
struct Pending {
    shown: FountainDevice,
    task: tokio::task::AbortHandle,
}

pub struct Account {
    dir: PathBuf,
    state_dir: PathBuf,
    /// What Fountain names the key ("Arugula on geek").
    client_name: String,
    shell_env: Option<Arc<ShellEnv>>,
    saved: Mutex<Option<Saved>>,
    pending: Mutex<Option<Pending>>,
    error: Mutex<Option<String>>,
    /// Bumped on every sign-in and sign-out: the stream starts again.
    pub(super) changed: watch::Sender<u64>,
    pub(crate) stream: Mutex<FountainStream>,
    hub: Arc<Hub>,
    /// One second; less in tests.
    tick: Duration,
}

static ACCOUNT: OnceLock<Arc<Account>> = OnceLock::new();

/// Set up the daemon's account at start-up.
pub fn install(state_dir: &Path, client_name: &str, shell_env: Arc<ShellEnv>) -> Arc<Account> {
    ACCOUNT
        .get_or_init(|| Arc::new(Account::open(state_dir, client_name, Some(shell_env), Duration::from_secs(1))))
        .clone()
}

pub fn get() -> Option<Arc<Account>> {
    ACCOUNT.get().cloned()
}

/// A URL as typed: `https://` added when there's no scheme, the trailing
/// slash taken off. Only http and https.
pub fn base_url(s: &str) -> Result<String, String> {
    let s = s.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("which Fountain? give its URL".into());
    }
    let s = if s.contains("://") { s.to_owned() } else { format!("https://{s}") };
    let u = url::Url::parse(&s).map_err(|e| format!("{s} isn't a URL: {e}"))?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return Err(format!("{s} isn't an http(s) URL"));
    }
    if !u.username().is_empty() || u.password().is_some() || u.query().is_some() {
        return Err(format!("{s}: just the instance's URL, please"));
    }
    Ok(s)
}

/// The base URL the CLI's login names, if it names one: `$FOUNTAIN_BASE_URL`,
/// else the profile's (`$FOUNTAIN_PROFILE`, else `default`).
pub fn named_base(found: &login::Found) -> Option<String> {
    let env = found.env_base.trim();
    if !env.is_empty() {
        return Some(env.trim_end_matches('/').to_owned());
    }
    let profile = Some(found.env_profile.trim()).filter(|p| !p.is_empty()).unwrap_or("default");
    let all = found.file.as_deref().map(login::parse_credentials).unwrap_or_default();
    all.get(profile)?.get("base_url").map(|b| b.trim().trim_end_matches('/').to_owned()).filter(|b| !b.is_empty())
}

impl Account {
    pub fn open(state_dir: &Path, client_name: &str, shell_env: Option<Arc<ShellEnv>>, tick: Duration) -> Self {
        let dir = state_dir.join("fountain");
        let file = dir.join("login.json");
        let saved = match std::fs::read(&file) {
            Ok(b) => match serde_json::from_slice::<Saved>(&b) {
                Ok(s) if !s.key.is_empty() && !s.base_url.is_empty() => Some(s),
                Ok(_) => None,
                Err(e) => {
                    warn!(file = %file.display(), error = %e, "Fountain login unreadable; signed out");
                    None
                }
            },
            Err(_) => None,
        };
        Self {
            dir,
            state_dir: state_dir.to_owned(),
            client_name: client_name.to_owned(),
            shell_env,
            saved: Mutex::new(saved),
            pending: Mutex::new(None),
            error: Mutex::new(None),
            changed: watch::channel(0).0,
            stream: Mutex::new(FountainStream::default()),
            hub: Arc::new(Hub::default()),
            tick,
        }
    }

    /// Where the key and the stream's cursor are kept.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// The account stream's subscribers subscribe here.
    pub fn hub(&self) -> Arc<Hub> {
        self.hub.clone()
    }

    /// Told on every sign-in and sign-out.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    fn file(&self) -> PathBuf {
        self.dir.join("login.json")
    }

    fn save(&self, s: &Saved) -> Result<(), String> {
        crate::store::private_dir(&self.dir).map_err(|e| format!("can't save the Fountain login: {e}"))?;
        let bytes = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
        crate::store::write_atomic(&self.file(), &bytes).map_err(|e| format!("can't save the Fountain login: {e}"))?;
        // A file made by an older daemon, or by hand, may be wider.
        crate::perm::set(&self.file(), 0o600).map_err(|e| format!("can't save the Fountain login: {e}"))
    }

    fn bump(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    /// This daemon's own sign-in, if there is one.
    pub fn signed_in(&self) -> Option<Resolved> {
        self.saved.lock().unwrap().as_ref().map(|s| Resolved {
            base_url: s.base_url.clone(),
            key: s.key.clone(),
            source: Source::Arugula,
            user_id: s.user_id.clone(),
        })
    }

    /// What the CLI's login on this host says (today's resolution).
    async fn found(&self) -> Result<login::Found, String> {
        let Some(shell_env) = &self.shell_env else { return Ok(login::Found::default()) };
        let runner = super::local_runner(shell_env).await;
        login::read(&runner).await
    }

    /// The login to use: this daemon's sign-in, else the CLI's.
    pub async fn resolve(&self) -> Result<Resolved, String> {
        if let Some(r) = self.signed_in() {
            return Ok(r);
        }
        let found = self.found().await?;
        let l = login::resolve(&found, None)
            .map_err(|e| format!("{e} (or sign this daemon in: `arugula fountain login URL`)"))?;
        Ok(Resolved {
            base_url: l.base_url,
            key: l.key,
            source: match l.from {
                login::From::Env => Source::Env,
                login::From::File => Source::File,
                login::From::Arugula => Source::Arugula,
            },
            user_id: None,
        })
    }

    /// What a client may know. Never the key.
    pub async fn status(&self) -> FountainLogin {
        let mut out = FountainLogin {
            pending: self.pending.lock().unwrap().as_ref().map(|p| p.shown.clone()),
            error: self.error.lock().unwrap().clone(),
            stream: self.stream.lock().unwrap().clone(),
            ..FountainLogin::default()
        };
        let saved = self.saved.lock().unwrap().clone();
        match saved {
            Some(s) => {
                out.signed_in = true;
                out.source = Some(Source::Arugula.name().into());
                out.base_url = Some(s.base_url);
                out.email = s.email;
                out.key_prefix = s.prefix;
            }
            None => match self.resolve().await {
                Ok(r) => {
                    out.signed_in = true;
                    out.source = Some(r.source.name().into());
                    out.base_url = Some(r.base_url);
                }
                Err(e) => {
                    out.error.get_or_insert(e);
                }
            },
        }
        out
    }

    /// Start a device sign-in on `base` (see the module's doc for the
    /// default). The answer's `pending` has the code; polling goes on in
    /// the background. Starting again drops the one before.
    pub async fn start(self: &Arc<Self>, base: Option<&str>) -> Result<FountainLogin, String> {
        let base = match base.map(str::trim).filter(|b| !b.is_empty()) {
            Some(b) => base_url(b)?,
            None => match self.signed_in().map(|s| s.base_url) {
                Some(b) => b,
                None => named_base(&self.found().await.unwrap_or_default()).ok_or_else(|| {
                    "which Fountain? give its URL: `arugula fountain login https://fountain.example`".to_owned()
                })?,
            },
        };
        let http = client::http(Some(Duration::from_secs(20)));
        let grant = client::device_start(&http, &base, &self.client_name).await.map_err(|e| e.to_string())?;
        let shown = FountainDevice {
            base_url: base.clone(),
            user_code: grant.user_code.clone(),
            verification_uri: grant.verification_uri.clone(),
            verification_uri_complete: grant.verification_uri_complete.clone(),
            expires_ms: now_ms() + grant.expires_in * 1000,
        };
        info!(base_url = %base, user_code = %grant.user_code, "Fountain sign-in started");
        let me = self.clone();
        let task = tokio::spawn(async move { me.poll(http, base, grant).await }).abort_handle();
        if let Some(old) = self.pending.lock().unwrap().replace(Pending { shown, task }) {
            old.task.abort();
        }
        *self.error.lock().unwrap() = None;
        Ok(self.status().await)
    }

    /// Poll until the grant is settled, then keep the key or say why not.
    async fn poll(self: Arc<Self>, http: reqwest::Client, base: String, grant: DeviceGrant) {
        let mut interval = grant.interval.max(1);
        let deadline = tokio::time::Instant::now() + self.tick * (grant.expires_in as u32);
        let said = loop {
            tokio::time::sleep(self.tick * interval as u32).await;
            if tokio::time::Instant::now() >= deadline {
                break Err("the code expired before it was approved: sign in again".to_owned());
            }
            match client::device_poll(&http, &base, &grant.device_code).await {
                Ok(Poll::Pending) => {}
                Ok(Poll::SlowDown) => interval += 5,
                Ok(Poll::Approved(k)) => break Ok(k),
                Ok(Poll::Denied) => break Err("the sign-in was denied".to_owned()),
                Ok(Poll::Expired) => break Err("the code expired before it was approved: sign in again".to_owned()),
                Ok(Poll::Invalid) => break Err("Fountain no longer knows that code: sign in again".to_owned()),
                // Fountain or the network, for now: the code stays good.
                Err(e) if e.transient() => warn!(error = %e, "Fountain sign-in: polling"),
                Err(e) => break Err(e.to_string()),
            }
        };
        let done = match said {
            Ok(k) => {
                let c = Client::new(&base, &k.api_key);
                let me = match c.me().await {
                    Ok(m) => Some(m),
                    Err(e) => {
                        warn!(error = %e, "Fountain sign-in: who is it");
                        None
                    }
                };
                let saved = Saved {
                    base_url: base.clone(),
                    key: k.api_key,
                    key_id: k.key_id,
                    prefix: k.prefix,
                    email: me.as_ref().and_then(|m| m.email.clone()),
                    user_id: me.map(|m| m.id),
                    at_ms: now_ms(),
                };
                match self.save(&saved) {
                    Ok(()) => {
                        info!(base_url = %base, prefix = ?saved.prefix, "signed in to Fountain");
                        *self.saved.lock().unwrap() = Some(saved);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        *self.pending.lock().unwrap() = None;
        if let Err(e) = &done {
            warn!(base_url = %base, error = %e, "Fountain sign-in");
        }
        *self.error.lock().unwrap() = done.err();
        self.bump();
    }

    /// Forget this daemon's key (and a sign-in in progress), revoking the
    /// key on Fountain when it can. The CLI's login is left alone.
    pub async fn logout(&self) -> Result<FountainLogin, String> {
        if let Some(p) = self.pending.lock().unwrap().take() {
            p.task.abort();
        }
        let gone = self.saved.lock().unwrap().take();
        match std::fs::remove_file(self.file()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("can't remove the Fountain login: {e}")),
        }
        *self.error.lock().unwrap() = None;
        self.bump();
        let note = match gone {
            None => Some("this daemon wasn't signed in to Fountain".to_owned()),
            Some(s) => match &s.key_id {
                Some(id) => match Client::new(&s.base_url, &s.key).revoke_key(id).await {
                    Ok(()) => Some("the key was revoked on Fountain".to_owned()),
                    Err(e) => Some(format!("Fountain didn't revoke the key ({e}): revoke it in its console")),
                },
                None => Some("revoke the key in Fountain's console".to_owned()),
            },
        };
        info!(note = ?note, "signed out of Fountain");
        let mut out = self.status().await;
        out.note = note;
        Ok(out)
    }
}

// ----------------------------------------------------------- the routes

/// `/api/fountain/login`, added to the API's router by
/// [`crate::labs::routes`]: operations (`ops/fountain_login.rs`).
pub fn routes(r: axum::Router<Arc<crate::server::App>>) -> axum::Router<Arc<crate::server::App>> {
    use crate::ops::OpRoutes;
    use arugula_proto::op::ops::{FountainLoginGet, FountainLoginStart, FountainLogout};
    r.op::<FountainLoginGet>().op::<FountainLoginStart>().op::<FountainLogout>()
}

fn installed() -> Result<Arc<Account>, crate::api::ApiError> {
    get().ok_or_else(|| {
        crate::api::ApiError(axum::http::StatusCode::SERVICE_UNAVAILABLE, "Fountain isn't set up yet".into())
    })
}

/// `GET /api/fountain/login`.
pub async fn fountain_login() -> Result<FountainLogin, crate::api::ApiError> {
    Ok(installed()?.status().await)
}

/// `POST /api/fountain/login`: a 400 says why it can't start (which
/// Fountain, or what Fountain said).
pub async fn fountain_login_start(
    req: arugula_proto::api::FountainLoginRequest,
) -> Result<FountainLogin, crate::api::ApiError> {
    installed()?.start(req.base_url.as_deref()).await.map_err(crate::api::bad)
}

/// `DELETE /api/fountain/login`.
pub async fn fountain_logout() -> Result<FountainLogin, crate::api::ApiError> {
    installed()?.logout().await.map_err(|e| crate::api::ApiError(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use axum::{
        Json, Router,
        extract::{Path as UrlPath, State},
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{delete, get, post},
    };
    use serde_json::{Value, json};

    use super::*;

    #[derive(Clone, Default)]
    struct Fake {
        polls: Arc<AtomicU32>,
        deny: bool,
        revoked: Arc<Mutex<Vec<(String, String)>>>,
    }

    async fn serve(f: Fake) -> String {
        async fn device() -> Response {
            (
                StatusCode::CREATED,
                Json(json!({ "device_code": "dc", "user_code": "WXYZ-1234", "verification_uri": "http://f/device",
                             "verification_uri_complete": "http://f/device?code=WXYZ-1234", "expires_in": 600, "interval": 1 })),
            )
                .into_response()
        }
        async fn token(State(f): State<Fake>) -> Response {
            let n = f.polls.fetch_add(1, Ordering::SeqCst) + 1;
            let v = match (n, f.deny) {
                (1, _) => json!({ "error": "authorization_pending" }),
                (2, _) => json!({ "error": "slow_down" }),
                (_, true) => json!({ "error": "access_denied" }),
                _ => {
                    return (
                        StatusCode::CREATED,
                        Json(json!({ "api_key": "ftn_secret_key", "key_id": "k9", "prefix": "ftn_se" })),
                    )
                        .into_response();
                }
            };
            (StatusCode::BAD_REQUEST, Json(v)).into_response()
        }
        async fn me(h: HeaderMap) -> Json<Value> {
            assert_eq!(h["authorization"], "Bearer ftn_secret_key");
            Json(json!({ "id": "u1", "email": "jake@example.com", "role": "user" }))
        }
        async fn revoke(State(f): State<Fake>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> StatusCode {
            let auth = h["authorization"].to_str().unwrap().to_owned();
            f.revoked.lock().unwrap().push((id, auth));
            StatusCode::NO_CONTENT
        }
        let app = Router::new()
            .route("/api/auth/device", post(device))
            .route("/api/auth/device/token", post(token))
            .route("/api/auth/me", get(me))
            .route("/api/auth/api-keys/{id}", delete(revoke))
            .with_state(f);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ilg-fountain-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn settled(a: &Account) {
        for _ in 0..400 {
            if a.pending.lock().unwrap().is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the sign-in never settled");
    }

    #[test]
    fn urls_as_typed() {
        assert_eq!(base_url("fountain.example/").unwrap(), "https://fountain.example");
        assert_eq!(base_url(" http://127.0.0.1:4000 ").unwrap(), "http://127.0.0.1:4000");
        assert!(base_url("").is_err() && base_url("ftp://x").is_err() && base_url("https://u:p@x").is_err());
        let found = |env_base: &str, file: Option<&str>| login::Found {
            env_base: env_base.into(),
            file: file.map(str::to_owned),
            ..Default::default()
        };
        assert_eq!(named_base(&found("http://e/", None)).as_deref(), Some("http://e"));
        assert_eq!(
            named_base(&found("", Some("[default]\nbase_url = \"https://p.example/\"\n"))).as_deref(),
            Some("https://p.example")
        );
        assert_eq!(named_base(&found("", Some("[default]\napi_key = \"k\"\n"))), None, "no default instance");
    }

    // Unix: checks the file's mode.
    #[cfg(unix)]
    #[tokio::test]
    async fn device_sign_in_keeps_the_key_private_and_sign_out_revokes_it() {
        let fake = Fake::default();
        let base = serve(fake.clone()).await;
        let dir = scratch("signin");
        let a = Arc::new(Account::open(&dir, "Arugula on test", None, Duration::from_millis(10)));
        let e = a.start(None).await.unwrap_err();
        assert!(e.contains("which Fountain?"), "{e}");
        let mut changes = a.changes();
        let s = a.start(Some(&base)).await.unwrap();
        let p = s.pending.unwrap();
        assert_eq!((p.user_code.as_str(), p.base_url.as_str()), ("WXYZ-1234", base.as_str()));
        assert!(!s.signed_in);
        settled(&a).await;
        assert!(changes.has_changed().unwrap());
        changes.mark_unchanged();
        assert_eq!(fake.polls.load(Ordering::SeqCst), 3, "pending, slow down, approved");
        let s = a.status().await;
        assert!(s.signed_in && s.error.is_none(), "{s:?}");
        assert_eq!(s.source.as_deref(), Some("arugula"));
        assert_eq!((s.email.as_deref(), s.key_prefix.as_deref()), (Some("jake@example.com"), Some("ftn_se")));
        let shown = serde_json::to_string(&s).unwrap();
        assert!(!shown.contains("ftn_secret_key"), "{shown}");
        let file = dir.join("fountain/login.json");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(dir.join("fountain")).unwrap().permissions().mode() & 0o777, 0o700);
        // Read back by the next daemon.
        let again = Arc::new(Account::open(&dir, "n", None, Duration::from_millis(10)));
        let r = again.resolve().await.unwrap();
        assert_eq!((r.source, r.key.as_str(), r.user_id.as_deref()), (Source::Arugula, "ftn_secret_key", Some("u1")));
        assert!(!format!("{r:?}").contains("ftn_secret_key"));
        // A second sign-in defaults to the instance signed in to.
        assert_eq!(again.start(None).await.unwrap().pending.unwrap().base_url, base);
        let out = again.logout().await.unwrap();
        assert_eq!(out.note.as_deref(), Some("the key was revoked on Fountain"));
        assert!(!out.signed_in && out.pending.is_none());
        assert!(!file.exists());
        assert_eq!(fake.revoked.lock().unwrap().clone(), [("k9".to_owned(), "Bearer ftn_secret_key".to_owned())]);
        assert!(again.resolve().await.is_err(), "no CLI login in a test");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_denied_sign_in_says_so() {
        let fake = Fake { deny: true, ..Fake::default() };
        let base = serve(fake).await;
        let dir = scratch("denied");
        let a = Arc::new(Account::open(&dir, "n", None, Duration::from_millis(10)));
        a.start(Some(&base)).await.unwrap();
        settled(&a).await;
        let s = a.status().await;
        assert!(!s.signed_in);
        assert_eq!(s.error.as_deref(), Some("the sign-in was denied"));
        assert!(!dir.join("fountain/login.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
