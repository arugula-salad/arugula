//! The GitHub App itself (M40, the platform's since M161): its JWT, where
//! it's installed, read-only installation tokens, and who may hear about a
//! repository through it ([`GithubApp::may`]).

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

/// How long an answer about an installation or a person's access holds.
const KEEP: Duration = Duration::from_secs(600);

/// What a hosted box's token may do: read.
fn read_only() -> Value {
    json!({
        "metadata": "read", "contents": "read", "pull_requests": "read", "issues": "read",
        "checks": "read", "statuses": "read", "actions": "read",
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub id: u64,
    /// The account it's installed on (a user or an organization).
    pub account: String,
    /// That account's numeric id.
    pub account_id: u64,
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
/// An installation token, GitHub's `expires_at`, and that in seconds.
type Minted = (String, String, u64);

pub struct GithubApp {
    pub id: String,
    pub slug: String,
    pub webhook_secret: String,
    /// The REST API's base (`https://api.github.com`; tests use a fake).
    pub api: String,
    key: KeyPair,
    installs: Mutex<HashMap<String, (Instant, Option<Install>)>>,
    access: Mutex<HashMap<(u64, String), Access>>,
    /// Installation tokens by (installation, repository), until they expire.
    tokens: Mutex<HashMap<(u64, String), Minted>>,
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
            tokens: Mutex::default(),
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
                })
            }
            404 => None,
            s => bail!("GitHub said {s} for the App's installation on {repo}"),
        };
        self.installs.lock().unwrap().insert(k, (Instant::now(), found.clone()));
        Ok(found)
    }

    /// An installation token for one repository (its name, without the
    /// owner), read-only: `(token, expires_at)`.
    pub async fn token(&self, http: &reqwest::Client, install: u64, name: &str) -> anyhow::Result<(String, String)> {
        let k = (install, name.to_ascii_lowercase());
        if let Some((t, at, exp)) = self.tokens.lock().unwrap().get(&k)
            && *exp > now_s() + 300
        {
            return Ok((t.clone(), at.clone()));
        }
        let r = self
            .req(
                http,
                reqwest::Method::POST,
                &format!("app/installations/{install}/access_tokens"),
                &self.jwt(now_s())?,
            )
            .json(&json!({ "repositories": [name], "permissions": read_only() }))
            .send()
            .await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or_default();
        if !status.is_success() {
            bail!("GitHub said {status} minting an installation token: {}", v["message"].as_str().unwrap_or(""));
        }
        let t = v["token"].as_str().context("no token in GitHub's answer")?.to_owned();
        let at = v["expires_at"].as_str().unwrap_or_default().to_owned();
        let exp = epoch_of(&at).unwrap_or(now_s() + 3600);
        let mut tokens = self.tokens.lock().unwrap();
        tokens.retain(|_, (_, _, e)| *e > now_s());
        tokens.insert(k, (t.clone(), at.clone(), exp));
        Ok((t, at))
    }

    /// Whether the answer for this person and repository is at hand.
    pub fn knows(&self, user: &GithubUser, repo: &str) -> bool {
        let k = (user.id, repo.to_ascii_lowercase());
        self.access.lock().unwrap().get(&k).is_some_and(|(at, _)| at.elapsed() < KEEP)
    }

    /// Whether `user` may hear about `repo` through the App: the
    /// installation that covers it, or why not.
    pub async fn may(&self, http: &reqwest::Client, user: &GithubUser, repo: &str) -> Result<Install, String> {
        let k = (user.id, repo.to_ascii_lowercase());
        if let Some((at, r)) = self.access.lock().unwrap().get(&k)
            && at.elapsed() < KEEP
        {
            return r.clone();
        }
        let r = self.may_now(http, user, repo).await;
        if let Err(e) = &r {
            info!(repo, why = e, "no live updates for a repository");
        }
        self.access.lock().unwrap().insert(k, (Instant::now(), r.clone()));
        r
    }

    async fn may_now(&self, http: &reqwest::Client, user: &GithubUser, repo: &str) -> Result<Install, String> {
        let login = user.login.as_str();
        let owner = repo.split('/').next().unwrap_or_default();
        let install = match self.installation(http, repo).await {
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
        let name = repo.split('/').nth(1).unwrap_or_default();
        let token = match self.token(http, install.id, name).await {
            Ok((t, _)) => t,
            Err(e) => {
                warn!(error = %e, "minting a token to check access");
                return Err("couldn't check your access to it with the App".into());
            }
        };
        let path = format!("repos/{repo}/collaborators/{login}/permission");
        let r = self.req(http, reqwest::Method::GET, &path, &token).send().await.map_err(|e| e.to_string())?;
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
