//! Control, as a native app reaches it: signing in with the account, as the
//! desktop app and the CLI do (aru-141, #999), and requests that go with the
//! session it gives.
//!
//! The app asks `/auth/app` for a ticket, naming itself a native app and its
//! callback (`arugula://signin`, Arugula #1247), with the challenge of a
//! verifier only it keeps. A signed-in browser that clicks Allow is sent to
//! the callback with a one-time grant, which the platform's web sheet hands
//! back to the app. The app swaps the grant and its verifier for a session:
//! any app can claim the scheme, so the grant alone redeems nothing. No key
//! and no certificate: control decides who reaches a machine ([`crate::link`]).

use std::fmt;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::net::Site;

/// Where the browser's Allow sends the grant: the app's own scheme.
pub const CALLBACK: &str = "arugula://signin";

/// The scheme of [`CALLBACK`], for the platform's web sheet.
pub const CALLBACK_SCHEME: &str = "arugula";

/// The native app API this build reads and sends (#603). Raise it with a
/// change to what the app calls on control that an older build can't
/// follow; control's `/control.json` names the oldest it still serves
/// (`native_api_min`), and an older build says to update the app.
pub const NATIVE_API: u64 = 1;

/// Control no longer serves this build of the app: it's older than
/// `/control.json`'s `native_api_min`.
#[derive(Debug)]
pub struct UpdateApp {
    pub url: String,
    pub needs: u64,
}

impl fmt::Display for UpdateApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Arugula cloud at {} needs a newer version of this app: update the app", self.url)
    }
}

impl std::error::Error for UpdateApp {}

/// Whether control serves this build: its `/control.json`, read before
/// anything else so an app it's left behind says so instead of failing on
/// what it can't read. A control that can't be reached isn't an answer.
fn serves_this_app(site: &Site, url: &str) -> Result<()> {
    let got = site.request("GET", arugula_control_wire::CONTROL_JSON, &[], &[])?;
    ensure!((200..300).contains(&got.status), "control answered {} to /control.json", got.status);
    let said: arugula_control_wire::ControlNative =
        serde_json::from_slice(&got.body).context("control's /control.json")?;
    serves(said.native_api_min, url)
}

fn serves(native_api_min: u64, url: &str) -> Result<()> {
    if native_api_min > NATIVE_API {
        return Err(UpdateApp { url: url.to_owned(), needs: native_api_min }.into());
    }
    Ok(())
}

/// Whether control at `url` serves this build of the app, before signing in
/// or as it opens: [`UpdateApp`] when it doesn't.
pub fn check_app(url: &str) -> Result<()> {
    let url = url.trim().trim_end_matches('/');
    serves_this_app(&Site::parse(url)?, url)
}

/// What control answers when it refuses: its status and words.
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    pub error: String,
    /// A code a client can act on (`page_only`, say).
    pub reason: Option<String>,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "control refused ({}): {}", self.status, self.error)
    }
}

impl std::error::Error for Refused {}

impl Refused {
    fn of(status: u16, got: &[u8]) -> Self {
        let v: Value = serde_json::from_slice(got).unwrap_or(Value::Null);
        let error = v["error"].as_str().map_or_else(|| String::from_utf8_lossy(got).into_owned(), str::to_owned);
        let reason = v["reason"].as_str().map(str::to_owned);
        Self { status, error, reason }
    }

    /// The session has ended (signed out elsewhere, or expired): sign in
    /// again. A route only a browser's page may call refuses with
    /// `page_only`, which isn't that.
    pub fn signed_out(&self) -> bool {
        self.status == 401 && self.reason.as_deref() != Some("page_only")
    }
}

/// What the app keeps (in the keychain): where it's signed in, and the
/// session control gave it.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub url: String,
    /// Control's own origin, which a request riding on the cookie names:
    /// its page's, which may not be the address the app reaches it at.
    #[serde(default)]
    pub origin: String,
    /// The session's cookie, `name=value`, as control set it.
    pub cookie: String,
    pub account: String,
    /// The account's login, or its name: what the app shows.
    #[serde(default)]
    pub login: String,
}

/// An app signed in to an account: every request goes with its session.
pub struct Control {
    pub url: String,
    pub(crate) site: Site,
    pub session: Session,
}

impl Control {
    pub fn new(session: Session) -> Result<Self> {
        let url = session.url.trim_end_matches('/').to_owned();
        let site = Site::parse(&url)?;
        Ok(Self { url, site, session })
    }

    /// Who's asking, for a request to control: the session's cookie, and
    /// control's own origin, which a change made with a cookie needs.
    pub(crate) fn auth(&self) -> [(&'static str, &str); 2] {
        let origin = if self.session.origin.is_empty() { &self.url } else { &self.session.origin };
        [("cookie", self.session.cookie.as_str()), ("origin", origin.as_str())]
    }

    /// A request as the account: its JSON, or [`Refused`].
    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let bytes = body.map(serde_json::to_vec).transpose()?.unwrap_or_default();
        let mut headers = self.auth().to_vec();
        if body.is_some() {
            headers.push(("Content-Type", "application/json"));
        }
        let got = self.site.request(method, path, &headers, &bytes)?;
        if !(200..300).contains(&got.status) {
            return Err(Refused::of(got.status, &got.body).into());
        }
        Ok(serde_json::from_slice(&got.body).unwrap_or(Value::Null))
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.call("GET", path, None)
    }

    /// A `GET` whose reply isn't JSON (a person's picture, Arugula #1420):
    /// its bytes as they came, or [`Refused`].
    pub fn bytes(&self, path: &str) -> Result<Vec<u8>> {
        let got = self.site.request("GET", path, &self.auth(), &[])?;
        if !(200..300).contains(&got.status) {
            return Err(Refused::of(got.status, &got.body).into());
        }
        Ok(got.body)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.call("POST", path, Some(body))
    }

    /// End the session at control. The app forgets it whatever this says.
    pub fn sign_out(&self) -> Result<()> {
        self.post("/auth/logout", &json!({})).map(drop)
    }
}

/// Control is older than a native app's sign-in (Arugula #1247), or doesn't
/// take native apps: it didn't answer `"kind": "native"`.
#[derive(Debug)]
pub struct OlderControl(pub String);

impl fmt::Display for OlderControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Arugula cloud at {} isn't taking the app's sign-in yet", self.0)
    }
}

impl std::error::Error for OlderControl {}

/// A sign-in waiting for a browser's Allow: control has the ticket, and the
/// app keeps the verifier.
pub struct SignIn {
    url: String,
    site: Site,
    ticket: String,
    verifier: String,
    origin: String,
    /// What the web sheet opens: control's page, where someone signed in
    /// clicks Allow.
    pub link: String,
    pub expires_in_secs: u64,
}

impl SignIn {
    /// Ask control at `url` for a ticket. `name` says what this is on the
    /// Allow page ("Arugula on Jake's iPhone").
    pub fn start(url: &str, name: &str) -> Result<Self> {
        let url = url.trim().trim_end_matches('/').to_owned();
        let site = Site::parse(&url)?;
        ensure!(
            site.tls || private_host(&site.host),
            "control's address must be https:// (or http on this network, for testing)"
        );
        // Anything else wrong with /control.json, the sign-in says better.
        if let Err(e) = serves_this_app(&site, &url)
            && e.is::<UpdateApp>()
        {
            return Err(e);
        }
        let verifier = hex::encode(random::<32>());
        let challenge = hex::encode(Sha256::digest(verifier.as_bytes()));
        let ask = json!({
            "name": name,
            "challenge": challenge,
            "features": ["no-code", "native"],
            "callback": CALLBACK,
        });
        let got = site
            .request("POST", "/auth/app", &[("Content-Type", "application/json")], &serde_json::to_vec(&ask)?)
            .with_context(|| format!("can't reach Arugula cloud at {url}"))?;
        // A control that takes no native apps refuses the ask outright.
        if got.status == 400 || got.status == 404 {
            return Err(OlderControl(url).into());
        }
        if !(200..300).contains(&got.status) {
            return Err(Refused::of(got.status, &got.body).into());
        }
        let t: Value = serde_json::from_slice(&got.body).context("control's answer to a sign-in")?;
        if t["kind"] != "native" {
            return Err(OlderControl(url).into());
        }
        let ticket = t["ticket"].as_str().filter(|t| !t.is_empty() && t.bytes().all(|c| c.is_ascii_hexdigit()));
        let ticket = ticket.context("control's answer has no ticket")?.to_owned();
        let link = t["url"].as_str().filter(|l| l.starts_with("https://") || l.starts_with("http://"));
        let link = link.context("control's answer has no link")?.to_owned();
        let origin = origin_of(&link).context("control's link has no origin")?;
        let expires_in_secs = t["expires_in_secs"].as_u64().unwrap_or(600);
        Ok(Self { url, site, ticket, verifier, origin, link, expires_in_secs })
    }

    /// The ticket control gave: the callback names it.
    pub fn ticket(&self) -> &str {
        &self.ticket
    }

    /// Swap the grant in `callback` (where the browser's Allow sent it) and
    /// the verifier for a session, and read whose it is.
    pub fn finish(&self, callback: &str) -> Result<Control> {
        let grant = grant_in(callback, &self.ticket).context("that isn't this sign-in's answer: start again")?;
        let body = json!({ "grant": grant, "verifier": self.verifier });
        // No origin: control takes a native app's ticket from the app that
        // asked.
        let got = self.site.request(
            "POST",
            &format!("/auth/app/{}/redeem", self.ticket),
            &[("Content-Type", "application/json")],
            &serde_json::to_vec(&body)?,
        )?;
        if !(200..300).contains(&got.status) {
            return Err(Refused::of(got.status, &got.body)).context("control didn't sign this app in");
        }
        let cookie = got.cookie.context("control signed this app in, but sent no session")?;
        let mut c = Control::new(Session {
            url: self.url.clone(),
            origin: self.origin.clone(),
            cookie,
            account: String::new(),
            login: String::new(),
        })?;
        let me = c.get("/api/me").context("reading who this app is signed in as")?;
        let account = me["account"].as_str().filter(|a| !a.is_empty()).context("control didn't say which account")?;
        c.session.account = account.to_owned();
        c.session.login =
            me["login"].as_str().filter(|l| !l.is_empty()).or(me["name"].as_str()).unwrap_or_default().to_owned();
        Ok(c)
    }
}

/// The grant in the callback address for `ticket`:
/// `arugula://signin?ticket=…&grant=…`.
fn grant_in(callback: &str, ticket: &str) -> Option<String> {
    let rest = callback.trim().strip_prefix(CALLBACK)?;
    let query = rest.strip_prefix('?')?.split('#').next().unwrap_or_default();
    let param = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='));
    if param("ticket")? != ticket {
        return None;
    }
    let grant = param("grant")?;
    (!grant.is_empty() && grant.len() <= 256 && grant.bytes().all(|c| c.is_ascii_hexdigit())).then(|| grant.to_owned())
}

/// `scheme://authority` of a link.
fn origin_of(link: &str) -> Option<String> {
    let (scheme, rest) = link.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().filter(|a| !a.is_empty())?;
    Some(format!("{scheme}://{authority}"))
}

/// Loopback or a private address (or `localhost`): plain http is allowed
/// there, for tests and labs.
fn private_host(host: &str) -> bool {
    match host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => v4.is_loopback() || v4.is_private(),
        Ok(std::net::IpAddr::V6(v6)) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        Err(_) => host == "localhost",
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the OS random source");
    b
}

/// A machine in control's directory, as a channel to it needs it.
#[derive(Clone, Debug)]
pub struct Machine {
    pub id: String,
    pub name: String,
    /// Its Noise key: control's word for it (#994), else the certificate
    /// listed beside it (a hosted machine's, or another account's chain).
    pub noise: [u8; 32],
}

impl Control {
    /// Machine `id` from control's directory, with the Noise key a channel
    /// to it is opened to.
    pub fn machine(&self, id: &str) -> Result<Machine> {
        let dir = self.get("/api/directory")?;
        let d = dir["daemons"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["id"] == id)
            .with_context(|| format!("{id} isn't one of the machines Arugula cloud lists for you"))?;
        let name = d["name"].as_str().unwrap_or(id).to_owned();
        let Some(noise) = listed_noise(d).as_deref().and_then(noise_key) else {
            bail!("Arugula cloud lists {name}, but gives no key to reach it with");
        };
        Ok(Machine { id: id.to_owned(), name, noise })
    }
}

/// A directory entry's Noise key, hex: its own, else the one in the
/// certificate beside it, as the page and the CLI take it (`tokenEntry`).
fn listed_noise(d: &Value) -> Option<String> {
    if let Some(n) = d["noise"].as_str() {
        return Some(n.to_owned());
    }
    if d.get("hosted").is_some_and(|h| !h.is_null()) {
        return d["hosted"]["cert"]["noise"].as_str().map(str::to_owned);
    }
    d["chain"]["certs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["device"] == d["id"] && c["kind"] == "daemon")
        .and_then(|c| c["noise"].as_str())
        .map(str::to_owned)
}

/// A Noise public key in hex, as control lists one.
fn noise_key(hex: &str) -> Option<[u8; 32]> {
    hex::decode(hex.trim()).ok()?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grant_is_taken_from_this_tickets_callback_only() {
        let ok = "arugula://signin?ticket=ab12&grant=00ff";
        assert_eq!(grant_in(ok, "ab12").as_deref(), Some("00ff"));
        assert_eq!(grant_in("arugula://signin?grant=00ff&ticket=ab12", "ab12").as_deref(), Some("00ff"));
        // Another ticket's, another scheme's, a grant that isn't hex, none.
        assert_eq!(grant_in(ok, "cd34"), None);
        assert_eq!(grant_in("https://evil.example/signin?ticket=ab12&grant=00ff", "ab12"), None);
        assert_eq!(grant_in("arugula://signinx?ticket=ab12&grant=00ff", "ab12"), None);
        assert_eq!(grant_in("arugula://signin?ticket=ab12&grant=zz", "ab12"), None);
        assert_eq!(grant_in("arugula://signin?ticket=ab12", "ab12"), None);
    }

    #[test]
    fn a_machines_noise_key_is_controls_word_else_its_certificate() {
        let k = "11".repeat(32);
        let own = json!({ "id": "m1", "noise": k });
        assert_eq!(listed_noise(&own).as_deref(), Some(k.as_str()));
        let hosted = json!({ "id": "m2", "hosted": { "cert": { "noise": k } } });
        assert_eq!(listed_noise(&hosted).as_deref(), Some(k.as_str()));
        let chained = json!({ "id": "m3", "chain": { "certs": [
            { "device": "other", "kind": "daemon", "noise": "22".repeat(32) },
            { "device": "m3", "kind": "browser", "noise": "33".repeat(32) },
            { "device": "m3", "kind": "daemon", "noise": k },
        ] } });
        assert_eq!(listed_noise(&chained).as_deref(), Some(k.as_str()));
        assert_eq!(listed_noise(&json!({ "id": "m4" })), None);
        assert_eq!(noise_key(&k), Some([0x11; 32]));
        assert_eq!(noise_key("11"), None);
    }

    #[test]
    fn requests_name_controls_own_origin() {
        assert_eq!(origin_of("https://app.arugula.io/#app=ab12").as_deref(), Some("https://app.arugula.io"));
        assert_eq!(origin_of("http://control.test:8443#app=1").as_deref(), Some("http://control.test:8443"));
        assert_eq!(origin_of("https:///x"), None);
    }

    #[test]
    fn plain_http_only_on_this_network() {
        assert!(private_host("127.0.0.1") && private_host("10.1.2.3") && private_host("localhost"));
        assert!(private_host("[::1]"));
        assert!(!private_host("8.8.8.8") && !private_host("control.arugula.io"));
    }

    #[test]
    fn an_app_older_than_control_serves_is_told_to_update() {
        assert!(serves(0, "u").is_ok() && serves(NATIVE_API, "u").is_ok());
        let e = serves(NATIVE_API + 1, "https://control.test").unwrap_err();
        let u = e.downcast_ref::<UpdateApp>().expect("UpdateApp");
        assert_eq!(u.needs, NATIVE_API + 1);
        assert!(e.to_string().contains("update the app"));
    }

    #[test]
    fn signed_out_isnt_a_page_only_route() {
        let r = |status, reason: Option<&str>| Refused { status, error: String::new(), reason: reason.map(Into::into) };
        assert!(r(401, None).signed_out());
        assert!(!r(401, Some("page_only")).signed_out());
        assert!(!r(403, None).signed_out());
    }
}
