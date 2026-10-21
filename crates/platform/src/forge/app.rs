//! The GitHub App itself (M40, the platform's since M161): its JWT, where
//! it's installed, installation tokens, and who may hear about a
//! repository through it ([`GithubApp::may`]).
//!
//! **Tokens** (M167, aru-085). [`GithubApp::mint`] is the only code that
//! asks GitHub for an installation token. Each one names one repository and
//! the least it needs: a [`Purpose`] reads ([`ReadToken`], kept until five
//! minutes before it expires), and a send's [`Writes`] writes one kind of
//! thing ([`WriteToken`], minted for that send, never kept, and revoked at
//! GitHub once its one call is made: [`WriteToken::use_once`]).
//! The two are different types, a read names a [`Purpose`] and a send a
//! [`Writes`], so a read can't come away with a token that writes:
//!
//! ```no_run
//! # use arugula_platform::forge::{Caller, GithubApp, Purpose, ReadToken, Repo, Writes, WriteToken};
//! async fn read(app: &GithubApp, c: &Caller<'_>) -> ReadToken {
//!     app.read_token(c, 11, Repo::Name("o/hud"), Purpose::Read).await.unwrap()
//! }
//! async fn send(app: &GithubApp, c: &Caller<'_>) -> WriteToken {
//!     app.write_token(c, 11, Repo::Id(101), Writes::Issues).await.unwrap()
//! }
//! ```
//! ```compile_fail
//! # use arugula_platform::forge::{Caller, GithubApp, Purpose, Repo, WriteToken};
//! async fn read(app: &GithubApp, c: &Caller<'_>) -> WriteToken {
//!     app.read_token(c, 11, Repo::Name("o/hud"), Purpose::Read).await.unwrap()
//! }
//! ```
//! ```compile_fail
//! # use arugula_platform::forge::{Caller, GithubApp, Purpose, Repo};
//! async fn read(app: &GithubApp, c: &Caller<'_>) {
//!     app.write_token(c, 11, Repo::Name("o/hud"), Purpose::Read).await.unwrap();
//! }
//! ```
//!
//! Every mint is a `github_token_mints` row ([`Db::log_mint`]): who it was
//! for, what for, the installation, the repository and the permissions,
//! never the token. A read token used again from the cache inside the
//! platform adds no row; one handed out again to leave the platform (a
//! hosted box's, [`GithubApp::hand_out`]) does, for whoever it went to,
//! with the purpose `read (cached)`.
//!
//! A token whose answer from GitHub doesn't cover exactly the one
//! repository asked for is revoked and refused (still logged: it was
//! minted).

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{Context as _, bail};
use aws_lc_rs::{rand::SystemRandom, rsa::KeyPair, signature::RSA_PKCS1_SHA256};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::db::{
    Db,
    forge::{MintKind, NewMint},
};

/// How long an answer about an installation or a person's access holds.
const KEEP: Duration = Duration::from_secs(600);

/// What a read token may do: read, and nothing else.
fn read_only() -> Value {
    json!({
        "metadata": "read", "contents": "read", "pull_requests": "read", "issues": "read",
        "checks": "read", "statuses": "read", "actions": "read",
    })
}

/// What a read-only installation token is for. Both get [`read_only`]
/// permissions; the purpose is what the log says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The mirror's own reads: backfill, the poller, a webhook's item (M164).
    Ingest,
    /// A read for a person: their access check, a hosted box's token, a
    /// body fetched to show them.
    Read,
}

/// What a send writes, so the one write permission its token gets (and
/// `metadata: read`, which GitHub adds to every token). Only a send names
/// one ([`GithubApp::write_token`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writes {
    /// An issue: a comment (on a PR's conversation too), its state, labels.
    Issues,
    /// A PR: a review, a review comment, its state.
    PullRequests,
    /// Only for one call GitHub needs both for.
    Both,
}

/// What a mint asks GitHub for, and what the log says it was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Read(Purpose),
    Send(Writes),
}

impl Scope {
    fn permissions(self) -> Value {
        match self {
            Scope::Read(_) => read_only(),
            Scope::Send(Writes::Issues) => json!({ "metadata": "read", "issues": "write" }),
            Scope::Send(Writes::PullRequests) => json!({ "metadata": "read", "pull_requests": "write" }),
            Scope::Send(Writes::Both) => json!({ "metadata": "read", "issues": "write", "pull_requests": "write" }),
        }
    }

    fn purpose(self) -> &'static str {
        match self {
            Scope::Read(Purpose::Ingest) => "ingest",
            Scope::Read(Purpose::Read) => "read",
            Scope::Send(Writes::Issues) => "send:issues",
            Scope::Send(Writes::PullRequests) => "send:pull_requests",
            Scope::Send(Writes::Both) => "send:issues+pull_requests",
        }
    }
}

/// The one repository a token is for, under the installation it's minted
/// on: GitHub's id where the caller has it (a linked repo, a webhook), or
/// `owner/name` (a repository asked about by name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repo<'a> {
    Id(u64),
    Name(&'a str),
}

impl Repo<'_> {
    fn key(self) -> String {
        match self {
            Repo::Id(id) => format!("#{id}"),
            Repo::Name(n) => n.to_ascii_lowercase(),
        }
    }

    /// The access token request's repository: exactly one.
    fn body(self, permissions: &Value) -> Value {
        match self {
            Repo::Id(id) => json!({ "repository_ids": [id], "permissions": permissions }),
            Repo::Name(n) => {
                let name = n.split('/').nth(1).unwrap_or(n);
                json!({ "repositories": [name], "permissions": permissions })
            }
        }
    }

    /// Whether GitHub's answer covers this repository and nothing else:
    /// its id, if so.
    fn covered_by(self, answer: &Value) -> Option<u64> {
        let [r] = answer["repositories"].as_array()?.as_slice() else { return None };
        let id = r["id"].as_u64()?;
        let ok = match self {
            Repo::Id(want) => id == want,
            Repo::Name(n) => r["full_name"].as_str().is_some_and(|f| f.eq_ignore_ascii_case(n)),
        };
        ok.then_some(id)
    }
}

/// Who a mint is for and where it's written down: every mint is a
/// `github_token_mints` row.
#[derive(Clone, Copy)]
pub struct Caller<'a> {
    pub http: &'a reqwest::Client,
    pub db: &'a Db,
    /// The account, or `platform` for the platform's own work.
    pub who: &'a str,
}

impl<'a> Caller<'a> {
    pub fn of(p: &'a crate::Platform, who: &'a str) -> Self {
        Self { http: &p.http, db: &p.db, who }
    }
}

/// A read-only installation token for one repository. No `Debug` of its
/// secret: a token is never printed.
#[derive(Clone)]
pub struct ReadToken {
    secret: String,
    /// GitHub's `expires_at`.
    pub expires_at: String,
    expires: u64,
    /// The installation and repository it covers, for the log.
    install: u64,
    repo_id: u64,
}

impl ReadToken {
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl std::fmt::Debug for ReadToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadToken").field("expires_at", &self.expires_at).finish_non_exhaustive()
    }
}

/// An installation token that writes one kind of thing to one repository,
/// for one send. Not `Clone`, never kept, and its secret only reaches the
/// one call [`WriteToken::use_once`] makes, after which it's revoked. One
/// dropped unused is revoked too, in the background.
pub struct WriteToken {
    /// `None` once it's been used.
    secret: Option<String>,
    send: Writes,
    http: reqwest::Client,
    api: String,
}

impl WriteToken {
    /// What it may write.
    pub fn send(&self) -> Writes {
        self.send
    }

    /// Make the send's one call with it, then revoke it at GitHub,
    /// whatever the call's answer was.
    pub async fn use_once<T>(mut self, call: impl AsyncFnOnce(&str) -> T) -> T {
        let secret = self.secret.take().unwrap_or_default();
        let out = call(&secret).await;
        revoke(&self.http, &self.api, &secret).await;
        out
    }
}

impl Drop for WriteToken {
    fn drop(&mut self) {
        if let Some(secret) = self.secret.take()
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            let (http, api) = (self.http.clone(), self.api.clone());
            rt.spawn(async move { revoke(&http, &api, &secret).await });
        }
    }
}

/// Revoke an installation token (`DELETE /installation/token`, as the
/// token itself): best effort, as it expires within the hour anyway.
async fn revoke(http: &reqwest::Client, api: &str, token: &str) {
    let r = http
        .delete(format!("{api}/installation/token"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "arugula-control")
        .send()
        .await;
    match r {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => warn!(status = %r.status(), "revoking an installation token"),
        Err(e) => warn!(error = %e, "revoking an installation token"),
    }
}

impl std::fmt::Debug for WriteToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteToken").field("send", &self.send).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub id: u64,
    /// The account it's installed on (a user or an organization).
    pub account: String,
    /// That account's numeric id.
    pub account_id: u64,
    /// `User` or `Organization`.
    pub account_type: String,
}

/// A repository as GitHub says it to the App, with what a person may do
/// on it ([`GithubApp::repo_role`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRole {
    pub id: u64,
    /// `OWNER/NAME` as GitHub spells it now.
    pub full_name: String,
    pub private: bool,
    /// GitHub's role for the person: `admin`, `maintain`, `write`,
    /// `triage` or `read`.
    pub role: String,
}

impl RepoRole {
    /// Whether the role may link the repo to a project (M163): an admin or
    /// a maintainer.
    pub fn may_link(&self) -> bool {
        matches!(self.role.as_str(), "admin" | "maintain")
    }
}

/// A person by their GitHub identity: the numeric id is who they are, the
/// login what control last saw them called. The hub knows it (they signed
/// in with GitHub) and says it with each call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubUser {
    pub id: u64,
    pub login: String,
}

/// When a person's access was looked at, and the answer.
type Access = (Instant, Result<Install, String>);

pub struct GithubApp {
    pub id: String,
    pub slug: String,
    pub webhook_secret: String,
    /// The REST API's base (`https://api.github.com`; tests use a fake).
    pub api: String,
    key: KeyPair,
    installs: Mutex<HashMap<String, (Instant, Option<Install>)>>,
    access: Mutex<HashMap<(u64, String), Access>>,
    /// Read tokens by (installation, repository), until five minutes
    /// before they expire. Only read tokens: a write token is never kept.
    reads: Mutex<HashMap<(u64, String), ReadToken>>,
}

/// An RSA private key from PEM (PKCS#1 `RSA PRIVATE KEY`, as GitHub hands
/// them out, or PKCS#8 `PRIVATE KEY`). A Fly secret pasted with `\n`s
/// works too.
pub fn parse_pem(pem: &str) -> anyhow::Result<KeyPair> {
    let pem = pem.replace("\\n", "\n");
    let begin = pem.find("-----BEGIN ").context("not a PEM key (no BEGIN line)")?;
    let rest = &pem[begin + 11..];
    let label_end = rest.find("-----").context("not a PEM key")?;
    let label = &rest[..label_end];
    let body = &rest[label_end + 5..];
    let end = body.find("-----END ").context("not a PEM key (no END line)")?;
    let b64: String = body[..end].chars().filter(|c| !c.is_whitespace()).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(b64).context("the PEM key isn't base64")?;
    let key = match label {
        "RSA PRIVATE KEY" => KeyPair::from_der(&der),
        "PRIVATE KEY" => KeyPair::from_pkcs8(&der),
        l => bail!("a {l} isn't an RSA private key"),
    };
    key.map_err(|e| anyhow::anyhow!("the GitHub App's private key: {e}"))
}

/// Seconds since the epoch for GitHub's `2026-10-03T12:34:56Z`.
pub fn epoch_of(t: &str) -> Option<u64> {
    let (d, rest) = t.split_once('T')?;
    let mut ymd = d.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, dd) = (ymd.next()??, ymd.next()??, ymd.next()??);
    let hms = rest.trim_end_matches('Z');
    let hms = hms.split(['+', '.']).next()?;
    let mut p = hms.split(':').map(|x| x.parse::<i64>().ok());
    let (h, mi, s) = (p.next()??, p.next()??, p.next()??);
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + dd - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + s).ok()
}

fn now_s() -> u64 {
    arugula_e2e::now_ms() / 1000
}

impl GithubApp {
    pub fn new(id: String, slug: String, webhook_secret: String, api: &str, key: KeyPair) -> Self {
        Self {
            id,
            slug,
            webhook_secret,
            api: api.trim_end_matches('/').to_owned(),
            key,
            installs: Mutex::default(),
            access: Mutex::default(),
            reads: Mutex::default(),
        }
    }

    /// The App's JWT: RS256, issued a minute ago (clock drift), for nine
    /// minutes (GitHub takes at most ten).
    pub fn jwt(&self, now: u64) -> anyhow::Result<String> {
        let header = B64.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let iss = self.id.parse::<u64>().map_or_else(|_| json!(self.id), |n| json!(n));
        let claims = B64.encode(json!({ "iat": now - 60, "exp": now + 540, "iss": iss }).to_string());
        let input = format!("{header}.{claims}");
        let mut sig = vec![0u8; self.key.public_modulus_len()];
        self.key
            .sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), input.as_bytes(), &mut sig)
            .map_err(|_| anyhow::anyhow!("signing the App's JWT"))?;
        Ok(format!("{input}.{}", B64.encode(sig)))
    }

    fn req(
        &self,
        http: &reqwest::Client,
        method: reqwest::Method,
        path: &str,
        bearer: &str,
    ) -> reqwest::RequestBuilder {
        http.request(method, format!("{}/{}", self.api, path.trim_start_matches('/')))
            .bearer_auth(bearer)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "arugula-control")
    }

    /// Where the App is installed for `repo`, if it is (kept ten minutes).
    pub async fn installation(&self, http: &reqwest::Client, repo: &str) -> anyhow::Result<Option<Install>> {
        let k = repo.to_ascii_lowercase();
        if let Some((at, i)) = self.installs.lock().unwrap().get(&k)
            && at.elapsed() < KEEP
        {
            return Ok(i.clone());
        }
        let r = self
            .req(http, reqwest::Method::GET, &format!("repos/{repo}/installation"), &self.jwt(now_s())?)
            .send()
            .await?;
        let found = match r.status().as_u16() {
            200 => {
                let v: Value = r.json().await?;
                let id = v["id"].as_u64().context("an installation with no id")?;
                Some(Install {
                    id,
                    account: v["account"]["login"].as_str().unwrap_or_default().to_owned(),
                    account_id: v["account"]["id"].as_u64().unwrap_or(0),
                    account_type: v["account"]["type"].as_str().unwrap_or_default().to_owned(),
                })
            }
            404 => None,
            s => bail!("GitHub said {s} for the App's installation on {repo}"),
        };
        self.installs.lock().unwrap().insert(k, (Instant::now(), found.clone()));
        Ok(found)
    }

    /// A read-only installation token for one repository, from the cache
    /// while it has more than five minutes left, or minted (and logged).
    pub async fn read_token(
        &self,
        c: &Caller<'_>,
        install: u64,
        repo: Repo<'_>,
        purpose: Purpose,
    ) -> anyhow::Result<ReadToken> {
        Ok(self.read_or_mint(c, install, repo, purpose).await?.0)
    }

    /// A read-only token to hand to someone outside the platform (a hosted
    /// box): as [`GithubApp::read_token`], and when it's the cache's, a row
    /// for who it went to now, `read (cached)`, since the mint's row names
    /// whoever it was first minted for.
    pub async fn hand_out(&self, c: &Caller<'_>, install: u64, repo: Repo<'_>) -> anyhow::Result<ReadToken> {
        let (t, minted) = self.read_or_mint(c, install, repo, Purpose::Read).await?;
        if !minted {
            c.db.log_mint(&NewMint {
                kind: MintKind::Installation,
                installation_id: Some(t.install),
                repo_id: Some(t.repo_id),
                permissions: &read_only(),
                caller: c.who,
                purpose: "read (cached)",
                at: arugula_e2e::now_ms(),
            })?;
        }
        Ok(t)
    }

    /// The cache's read token, or a new one: whether it was minted now.
    async fn read_or_mint(
        &self,
        c: &Caller<'_>,
        install: u64,
        repo: Repo<'_>,
        purpose: Purpose,
    ) -> anyhow::Result<(ReadToken, bool)> {
        let k = (install, repo.key());
        if let Some(t) = self.reads.lock().unwrap().get(&k)
            && t.expires > now_s() + 300
        {
            return Ok((t.clone(), false));
        }
        let (secret, expires_at, repo_id) = self.mint(c, install, repo, Scope::Read(purpose)).await?;
        let expires = epoch_of(&expires_at).unwrap_or(now_s() + 3600);
        let t = ReadToken { secret, expires_at, expires, install, repo_id };
        let mut reads = self.reads.lock().unwrap();
        reads.retain(|_, t| t.expires > now_s());
        reads.insert(k, t.clone());
        Ok((t, true))
    }

    /// A token that writes what `send` says to one repository, minted now
    /// for this send (and logged), never from or into the cache.
    pub async fn write_token(
        &self,
        c: &Caller<'_>,
        install: u64,
        repo: Repo<'_>,
        send: Writes,
    ) -> anyhow::Result<WriteToken> {
        let (secret, _, _) = self.mint(c, install, repo, Scope::Send(send)).await?;
        Ok(WriteToken { secret: Some(secret), send, http: c.http.clone(), api: self.api.clone() })
    }

    /// The only call to GitHub's `POST /app/installations/{id}/access_tokens`:
    /// a token for `repo` alone with `scope`'s permissions, written to the
    /// log as soon as GitHub hands it over, and revoked if GitHub's answer
    /// doesn't cover exactly that repository. `(token, expires_at, repo_id)`.
    async fn mint(
        &self,
        c: &Caller<'_>,
        install: u64,
        repo: Repo<'_>,
        scope: Scope,
    ) -> anyhow::Result<(String, String, u64)> {
        let permissions = scope.permissions();
        let r = self
            .req(
                c.http,
                reqwest::Method::POST,
                &format!("app/installations/{install}/access_tokens"),
                &self.jwt(now_s())?,
            )
            .json(&repo.body(&permissions))
            .send()
            .await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or_default();
        if !status.is_success() {
            bail!("GitHub said {status} minting an installation token: {}", v["message"].as_str().unwrap_or(""));
        }
        let t = v["token"].as_str().context("no token in GitHub's answer")?.to_owned();
        // GitHub says which repositories it covers: the one asked for.
        let covers = repo.covered_by(&v);
        let repo_id = covers.or(match repo {
            Repo::Id(id) => Some(id),
            Repo::Name(_) => None,
        });
        let logged = c.db.log_mint(&NewMint {
            kind: MintKind::Installation,
            installation_id: Some(install),
            repo_id,
            permissions: &permissions,
            caller: c.who,
            purpose: scope.purpose(),
            at: arugula_e2e::now_ms(),
        });
        let Some(repo_id) = covers.filter(|_| logged.is_ok()) else {
            revoke(c.http, &self.api, &t).await;
            logged?;
            bail!("GitHub's installation token doesn't cover just the repository asked for");
        };
        // Who and what; never the token.
        info!(installation = install, repo_id, caller = c.who, purpose = scope.purpose(), "installation token minted");
        Ok((t, v["expires_at"].as_str().unwrap_or_default().to_owned(), repo_id))
    }

    /// Whether the answer for this person and repository is at hand.
    pub fn knows(&self, user: &GithubUser, repo: &str) -> bool {
        let k = (user.id, repo.to_ascii_lowercase());
        self.access.lock().unwrap().get(&k).is_some_and(|(at, _)| at.elapsed() < KEEP)
    }

    /// Whether `user` may hear about `repo` through the App: the
    /// installation that covers it, or why not.
    pub async fn may(&self, c: &Caller<'_>, user: &GithubUser, repo: &str) -> Result<Install, String> {
        let k = (user.id, repo.to_ascii_lowercase());
        if let Some((at, r)) = self.access.lock().unwrap().get(&k)
            && at.elapsed() < KEEP
        {
            return r.clone();
        }
        let r = self.may_now(c, user, repo).await;
        if let Err(e) = &r {
            info!(repo, why = e, "no live updates for a repository");
        }
        self.access.lock().unwrap().insert(k, (Instant::now(), r.clone()));
        r
    }

    /// [`may`](Self::may), asked of GitHub now and not kept here: the
    /// viewer check (M165) keeps its answers in the platform's database,
    /// where every machine drops them on GitHub's events.
    pub async fn may_now(&self, c: &Caller<'_>, user: &GithubUser, repo: &str) -> Result<Install, String> {
        let login = user.login.as_str();
        let owner = repo.split('/').next().unwrap_or_default();
        let install = match self.installation(c.http, repo).await {
            Ok(Some(i)) => i,
            Ok(None) => {
                return Err(format!(
                    "the GitHub App {} isn't installed on {owner}: https://github.com/apps/{}/installations/new",
                    self.slug, self.slug
                ));
            }
            Err(e) => {
                warn!(error = %e, "asking GitHub about the App's installation");
                return Err("couldn't ask GitHub about the App's installation".into());
            }
        };
        // Their own: by id, as a login that changed hands would match.
        if install.account_id != 0 && install.account_id == user.id {
            return Ok(install);
        }
        // Someone else's installation (an organization's, say): only if
        // GitHub says they're a collaborator on the repository.
        let token = match self.read_token(c, install.id, Repo::Name(repo), Purpose::Read).await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "minting a token to check access");
                return Err("couldn't check your access to it with the App".into());
            }
        };
        let path = format!("repos/{repo}/collaborators/{login}/permission");
        let r =
            self.req(c.http, reqwest::Method::GET, &path, token.secret()).send().await.map_err(|e| e.to_string())?;
        let v: Value = if r.status().is_success() { r.json().await.unwrap_or_default() } else { Value::Null };
        // The login control knows may belong to someone else by now.
        if v["user"]["id"].as_u64().is_some_and(|id| id != user.id) {
            return Err(format!("the GitHub login {login} isn't yours any more: sign in to control with GitHub again"));
        }
        match (v["permission"].as_str(), v["user"]["id"].as_u64()) {
            (Some(p), Some(_)) if p != "none" => Ok(install),
            _ => Err(format!("GitHub doesn't list {login} as a collaborator on {repo}")),
        }
    }

    /// [`installation`](Self::installation), asked of GitHub now: a person
    /// who just installed the App is waiting on the answer.
    pub async fn installation_now(&self, http: &reqwest::Client, repo: &str) -> anyhow::Result<Option<Install>> {
        self.installs.lock().unwrap().remove(&repo.to_ascii_lowercase());
        self.installation(http, repo).await
    }

    /// Where a person installs the App (or picks the repos an installation
    /// has): GitHub's page, with `state`, which GitHub hands back to the
    /// App's setup URL afterwards.
    pub fn install_url(&self, state: &str) -> String {
        let state: String = url::form_urlencoded::byte_serialize(state.as_bytes()).collect();
        format!("https://github.com/apps/{}/installations/new?state={state}", self.slug)
    }

    /// `repo` through `install`, and `user`'s role on it, by their numeric
    /// id: what linking checks (M163). Asked with a read token for the
    /// repository ([`Purpose::Read`], logged as `c`'s when it's minted).
    /// `Err` is why not, for the person.
    pub async fn repo_role(
        &self,
        c: &Caller<'_>,
        install: &Install,
        user: &GithubUser,
        repo: &str,
    ) -> Result<RepoRole, String> {
        let http = c.http;
        let token = match self.read_token(c, install.id, Repo::Name(repo), Purpose::Read).await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "minting a token to check a role");
                return Err("couldn't check your role on it with the App".into());
            }
        };
        let get = |path: String| {
            let token = token.secret();
            async move {
                let r = self.req(http, reqwest::Method::GET, &path, token).send().await.map_err(|e| e.to_string())?;
                let ok = r.status().is_success();
                let v: Value = r.json().await.unwrap_or_default();
                Ok::<_, String>(ok.then_some(v))
            }
        };
        let Some(r) = get(format!("repos/{repo}")).await? else {
            return Err(format!("the App can't see {repo}: add it to the App's installation on GitHub"));
        };
        let (Some(id), Some(full_name)) = (r["id"].as_u64(), r["full_name"].as_str()) else {
            return Err(format!("GitHub didn't say what {repo} is"));
        };
        let login = &user.login;
        let p = get(format!("repos/{full_name}/collaborators/{login}/permission")).await?.unwrap_or_default();
        // The login control knows may belong to someone else by now.
        if p["user"]["id"].as_u64().is_some_and(|id| id != user.id) {
            return Err(format!("the GitHub login {login} isn't yours any more: sign in to control with GitHub again"));
        }
        let role = match (p["role_name"].as_str(), p["permission"].as_str(), p["user"]["id"].as_u64()) {
            (Some(r), _, Some(_)) => r.to_owned(),
            (None, Some(r), Some(_)) => r.to_owned(),
            _ => return Err(format!("GitHub doesn't list {login} as a collaborator on {full_name}")),
        };
        Ok(RepoRole { id, full_name: full_name.to_owned(), private: r["private"].as_bool().unwrap_or(true), role })
    }

    /// Access answers kept here that `gone` picks, by GitHub id and
    /// repository (`OWNER/NAME` in lower case): GitHub said who may see
    /// them changed (M165).
    pub fn forget_where(&self, gone: impl Fn(u64, &str) -> bool) {
        self.access.lock().unwrap().retain(|(user, repo), _| !gone(*user, repo));
    }

    /// Installations changed (an install, a repository added or removed).
    pub fn forget(&self) {
        self.installs.lock().unwrap().clear();
        self.access.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::{
        encoding::AsDer,
        rsa::KeySize,
        signature::{KeyPair as _, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey},
    };

    use super::*;

    fn throwaway_key() -> (String, Vec<u8>) {
        let k = KeyPair::generate(KeySize::Rsa2048).unwrap();
        let der = AsDer::<aws_lc_rs::encoding::Pkcs8V1Der>::as_der(&k).unwrap();
        let b = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
        let lines: Vec<String> = b.as_bytes().chunks(64).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
        (
            format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n", lines.join("\n")), // gitleaks:allow (a throwaway test key's PEM frame)
            k.public_key().as_ref().to_vec(),
        )
    }

    #[test]
    fn jwt_is_rs256_and_checks_out() {
        let (pem, public) = throwaway_key();
        let app =
            GithubApp::new("5171453".into(), "arugula-test".into(), "s".into(), "http://x", parse_pem(&pem).unwrap());
        let t = app.jwt(1_800_000_000).unwrap();
        let parts: Vec<&str> = t.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: Value = serde_json::from_slice(&B64.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header, json!({ "alg": "RS256", "typ": "JWT" }));
        let claims: Value = serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims, json!({ "iat": 1_800_000_000u64 - 60, "exp": 1_800_000_000u64 + 540, "iss": 5171453 }));
        let sig = B64.decode(parts[2]).unwrap();
        let input = format!("{}.{}", parts[0], parts[1]);
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &public).verify(input.as_bytes(), &sig).unwrap();
        // A tampered claim doesn't.
        let bad = format!("{}.{}", parts[0], B64.encode(br#"{"iss":1}"#));
        assert!(UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &public).verify(bad.as_bytes(), &sig).is_err());
        // As a Fly secret with literal \n, too; and not anything else.
        assert!(parse_pem(&pem.replace('\n', "\\n")).is_ok());
        assert!(parse_pem("-----BEGIN EC PRIVATE KEY-----\nAAAA\n-----END EC PRIVATE KEY-----").is_err()); // gitleaks:allow (a throwaway test key's PEM frame)
        assert!(parse_pem("nope").is_err());
    }

    #[test]
    fn times() {
        assert_eq!(epoch_of("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_of("2026-10-03T12:00:00Z"), Some(1_791_028_800));
        assert_eq!(epoch_of("2000-02-29T00:00:01Z"), Some(951_782_401));
        assert_eq!(epoch_of("junk"), None);
    }
}
