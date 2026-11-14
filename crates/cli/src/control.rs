//! The CLI as a client of your account on Arugula control.
//!
//! `arugula login` (aru-141, #998) signs this CLI in the way the desktop
//! app signs in (control's `/auth/app`): it makes a verifier it keeps,
//! listens on a loopback port and prints control's link. A signed-in
//! browser that clicks Allow hands a one-time grant to that port (or the
//! person pastes the address the browser lands on, when it's on another
//! computer), and the CLI swaps the grant and its verifier for a session of
//! the account, kept in the config dir (`cli-session.json`). No key and no
//! certificate: control decides who reaches a machine.
//!
//! Then `--host NAME` finds names that aren't in the local daemon's host
//! list in control's directory, and opens a Noise channel to the machine
//! with a key made for that channel alone and a machine token from control
//! (`POST /api/machines/{id}/token`) in its first message: straight to a URL
//! the machine lists, else through control's relay. The daemon checks the
//! token and nothing else about the CLI. The machine's Noise key is the one
//! control's directory gives (#994), else the certificate listed beside it.
//!
//! A CLI logged in before that is a `cli` device of the account (M49, with
//! `cli-key` and `cli-control.json`): it signs its requests to control with
//! its key (`x-arugula-auth`, as daemons sign theirs) and asks for machine
//! tokens the same way. Only when control gives no tokens at all (404: no
//! issuer key, as a dev control has none) or the machine's daemon is from
//! before them does it open the channel with its key, checked against the
//! account's pinned root (another account's machine against that account's
//! root, pinned the first time this CLI sees it), as before. `arugula login
//! --device` still makes one, until the key code goes (#1007).
//!
//! Each HTTP connection the commands make is a request carried over that
//! one channel ([`Link::stream`]), so `run`, `ls`, `capture` and the rest
//! work unchanged. An answer with no end (`events --follow`, `tail
//! --follow`) comes back in parts as it's written. A WebSocket to `/ws`
//! (`attach`) is the channel's own protocol messages, which the
//! channel already carries: the daemon makes each channel a client of its
//! own, so one `/ws` per command.

#[cfg(unix)]
use std::os::unix::net::UnixStream;
/// Windows: the HTTP client's end of a carried connection is a loopback
/// TCP pair ([`pair`]).
#[cfg(not(unix))]
type UnixStream = std::net::TcpStream;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufRead, BufReader, ErrorKind, IsTerminal, Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use arugula_control_wire::{
    self as wire,
    machine_token::{TokenHello, token_path},
    maintenance,
};
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Revocation, Trust,
    cert::{join_proof_body, request_auth},
    channel::{Channel, Initiator, Msg, Refusal, RequestHead, prologue},
    hosted::HeldClosures,
    keys::fingerprint,
    now_ms,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use crate::http::{Stream, Target, Url, json_answer, send_on};

const KEY_FILE: &str = "cli-key";
const STATE_FILE: &str = "cli-control.json";
/// A session from a browser's Allow (aru-141).
const SESSION_FILE: &str = "cli-session.json";
/// How long a direct URL gets before the relay is tried.
const DIRECT_TIMEOUT: Duration = Duration::from_secs(3);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

fn config_dir() -> PathBuf {
    arugula_proto::dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config/arugula"))
}

/// What `arugula login --device` pinned.
#[derive(Serialize, Deserialize)]
struct Saved {
    url: String,
    cert: Cert,
    trust: Trust,
    /// Other accounts' roots (their team's or shared machines), by account,
    /// as first seen.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pins: BTreeMap<String, String>,
    /// Control's word of delegations that ended, as first heard: it stays
    /// when control leaves it out.
    #[serde(default, skip_serializing_if = "HeldClosures::is_empty")]
    closed: HeldClosures,
}

impl Saved {
    fn write(&self) -> anyhow::Result<()> {
        write_private(STATE_FILE, self)
    }
}

/// What `arugula login` keeps since aru-141: a session of the account,
/// from a browser that allowed it, as the desktop app's window has one.
#[derive(Serialize, Deserialize)]
struct SavedSession {
    url: String,
    /// The session's cookie, `name=value`, as control set it.
    cookie: String,
    account: String,
    #[serde(default)]
    login: String,
}

/// `value` as JSON in the config dir's `file`, readable by this user only.
fn write_private<T: Serialize>(file: &str, value: &T) -> anyhow::Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{file}.tmp"));
    let mut f = fs::OpenOptions::new();
    f.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut f, 0o600);
    f.open(&tmp)?.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    fs::rename(&tmp, dir.join(file))?;
    Ok(())
}

/// This CLI, logged in.
pub struct Enrolled {
    /// The control it's logged in to (Arugula #681: `arugula access` asks
    /// it for a machine's powers only when the machine is joined there).
    pub(crate) url: String,
    account: String,
    how: How,
}

/// How this CLI is signed in.
enum How {
    /// A session from a browser's Allow (aru-141): its cookie.
    Session(String),
    /// A `cli` device of the account (M49), from before: its key signs
    /// requests to control, and opens a channel when tokens can't (#1007
    /// removes it).
    Device(Box<Device>),
}

/// A `cli` device's key and what it pinned.
struct Device {
    cert: Cert,
    trust: Trust,
    pins: BTreeMap<String, String>,
    closed: HeldClosures,
    keys: DeviceKeys,
}

/// `ARUGULA_DEBUG_HTTP=1`: each call to control says on stderr how long it
/// took and what control's `Server-Timing` said.
fn debug_http() -> bool {
    std::env::var("ARUGULA_DEBUG_HTTP").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Control gave no machine token: signed out, not let in to the machine,
/// or (404) a control that gives none at all or doesn't know the machine.
#[derive(Debug)]
struct NoToken {
    status: u16,
    said: String,
}

impl std::fmt::Display for NoToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Arugula cloud gave no token for this machine: {}", self.said)
    }
}

impl std::error::Error for NoToken {}

/// The machine's answer to a channel opened with a machine token: a
/// refusal, with its code (`machine-token`: the token wasn't taken;
/// `malformed`: a daemon from before machine tokens).
#[derive(Debug)]
struct Refused {
    code: String,
    message: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.code == "malformed" {
            write!(f, "the machine's arugulad is older than machine tokens (0.70.4): update it there")
        } else {
            write!(f, "the machine refused its token: {}", self.message)
        }
    }
}

impl std::error::Error for Refused {}

/// Whether a device should reach the machine with its key after trying a
/// token, as the page decides (`keyAfterTokens`): control gives no tokens
/// at all, or the machine's daemon is from before them. Anything else
/// (signed out, not let in, a refused token) the key wouldn't fix.
fn key_after_tokens(e: &anyhow::Error) -> bool {
    e.downcast_ref::<NoToken>().is_some_and(|n| n.status == 404)
        || e.downcast_ref::<Refused>().is_some_and(|r| r.code == "malformed")
}

/// The machine (or control for it) answered, so no other route would do
/// better.
fn answered(e: &anyhow::Error) -> bool {
    e.downcast_ref::<NoToken>().is_some() || e.downcast_ref::<Refused>().is_some()
}

/// Request headers this CLI adds, by name.
type Headers = Vec<(&'static str, String)>;

/// A Noise public key in hex, as control lists one.
fn noise_key(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

impl Enrolled {
    pub fn load() -> anyhow::Result<Option<Self>> {
        let dir = config_dir();
        if let Ok(text) = fs::read_to_string(dir.join(SESSION_FILE)) {
            let s: SavedSession =
                serde_json::from_str(&text).with_context(|| format!("reading {}", dir.join(SESSION_FILE).display()))?;
            return Ok(Some(Self { url: s.url, account: s.account, how: How::Session(s.cookie) }));
        }
        let Ok(text) = fs::read_to_string(dir.join(STATE_FILE)) else { return Ok(None) };
        let s: Saved =
            serde_json::from_str(&text).with_context(|| format!("reading {}", dir.join(STATE_FILE).display()))?;
        let keys = DeviceKeys::load(&dir.join(KEY_FILE))?;
        if keys.id() != s.cert.device {
            bail!("{} and {} don't match: `arugula logout`, then log in again", KEY_FILE, STATE_FILE);
        }
        Ok(Some(Self::device(s.url, Device { cert: s.cert, trust: s.trust, pins: s.pins, closed: s.closed, keys })))
    }

    fn device(url: String, d: Device) -> Self {
        Self { url, account: d.trust.account.clone(), how: How::Device(Box::new(d)) }
    }

    /// This CLI's key and pins, if it's a device of the account.
    fn dev(&self) -> Option<&Device> {
        match &self.how {
            How::Device(d) => Some(d),
            How::Session(_) => None,
        }
    }

    fn dev_mut(&mut self) -> Option<&mut Device> {
        match &mut self.how {
            How::Device(d) => Some(d),
            How::Session(_) => None,
        }
    }

    /// Save what a device pinned since it was loaded.
    fn save_device(&self) -> anyhow::Result<()> {
        let Some(d) = self.dev() else { return Ok(()) };
        Saved {
            url: self.url.clone(),
            cert: d.cert.clone(),
            trust: d.trust.clone(),
            pins: d.pins.clone(),
            closed: d.closed.clone(),
        }
        .write()
    }

    fn url(&self) -> anyhow::Result<Url> {
        Url::parse(&self.url)
    }

    /// A CLI enrolled at `url` with a fresh key, for a command's test
    /// against a server of the test's own.
    #[cfg(test)]
    pub(crate) fn for_tests(url: String) -> Self {
        let keys = DeviceKeys::generate();
        Self::device(
            url,
            Device {
                cert: Cert::new(&keys, "a", Kind::Cli, "cli"),
                trust: Trust { account: "a".into(), root: String::new() },
                pins: BTreeMap::new(),
                closed: Default::default(),
                keys,
            },
        )
    }

    /// A CLI signed in at `url` with a session's `cookie`, for a test.
    #[cfg(test)]
    pub(crate) fn for_tests_session(url: String, cookie: &str) -> Self {
        Self { url, account: "a".into(), how: How::Session(cookie.into()) }
    }

    /// Control's URL, as this CLI logged in to it.
    pub fn control_url(&self) -> &str {
        &self.url
    }

    /// The account this CLI is logged in as.
    pub fn account(&self) -> &str {
        &self.account
    }

    /// Control's origin, as its own page sends it: what a request riding on
    /// a session's cookie says it's from.
    fn origin(&self) -> String {
        self.url.trim_end_matches('/').to_owned()
    }

    /// Who's asking, for a request to control: the session's cookie (and
    /// control's own origin, which a change made with a cookie needs), or
    /// the device's signature over the request.
    fn auth_headers(&self, method: &str, path: &str, body: &[u8]) -> Headers {
        match &self.how {
            How::Session(cookie) => vec![("Cookie", cookie.clone()), ("Origin", self.origin())],
            How::Device(d) => vec![("x-arugula-auth", request_auth(&d.keys, method, path, body))],
        }
    }

    /// A request to control, as this CLI. It waits out a maintenance window
    /// on control's chat and Cloud routes (M141, `grants` is one) for up to
    /// five minutes, saying so on stderr; which 503s are sent again is
    /// [`maintenance::may_retry`]'s.
    pub(crate) fn api(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<Value> {
        let (status, got) = self.api_raw(method, path, body)?;
        if status == 401 && matches!(self.how, How::Session(_)) {
            bail!(
                "this CLI's sign-in to Arugula cloud at {} has ended: `arugula logout`, then `arugula login --browser` again",
                self.url
            );
        }
        json_answer(status, got).map_err(|e| e.context(format!("control at {}", self.url)))
    }

    /// [`api`](Self::api)'s answer as it came, status and body: for one
    /// that isn't JSON (an export as Markdown), or a status to tell apart.
    pub(crate) fn api_raw(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<(u16, Vec<u8>)> {
        let bytes = body.map(|b| b.to_string()).unwrap_or_default();
        let u = self.url()?;
        let mut waits = maintenance::Waits::new(maintenance::Patience::AGENT);
        loop {
            // Signed afresh each try: a signature's nonce counts once.
            let auth = self.auth_headers(method, path, bytes.as_bytes());
            // It reads the desktop app's `app` certificates (control
            // leaves them out for CLIs from before them).
            let mut headers = vec![("Content-Type", "application/json"), (wire::FEATURES_HEADER, wire::APP_DEVICES)];
            headers.extend(auth.iter().map(|(k, v)| (*k, v.as_str())));
            let began = std::time::Instant::now();
            let res = send_on(
                u.connect(Some(Duration::from_secs(10)))?,
                method,
                path,
                &u.authority,
                &headers,
                bytes.as_bytes(),
            )
            .with_context(|| format!("asking control at {}", self.url))?;
            let (status, asked) = (res.status, maintenance::retry_after(res.header("retry-after")));
            let timing = res.header("server-timing").map(str::to_owned);
            let got = res.bytes().with_context(|| format!("reading control at {}", self.url))?;
            if debug_http() {
                // Where the time went (docs/control-ops.md, "Where a
                // request's time goes"): the whole call as this CLI saw
                // it, connecting included, and control's Server-Timing.
                eprintln!(
                    "arugula: {method} {path}: {status} in {:.1} ms; server-timing: {}",
                    began.elapsed().as_secs_f64() * 1e3,
                    timing.as_deref().unwrap_or("(none)")
                );
            }
            if maintenance::may_retry(method, status, &got)
                && let Some(wait) = waits.next(asked)
            {
                eprintln!("arugula: control is busy (maintenance, most likely): trying again in {}s", wait.as_secs());
                std::thread::sleep(wait);
                continue;
            }
            if maintenance::is_maintenance(status, &got) {
                bail!("{}", maintenance::MAINTENANCE_SAID);
            }
            return Ok((status, got));
        }
    }

    /// The machines control lists: the account's own, its teams', those
    /// shared with it and the cloud workspaces of its projects. Control
    /// decides who reaches them (aru-141), so each is listed as control
    /// gives it, with its Noise key: nothing is checked against a root
    /// unless a device has to use its key ([`open`](Self::open)).
    pub fn directory(&mut self) -> anyhow::Result<Vec<Listed>> {
        let v = self.api("GET", "/api/directory", None)?;
        Ok(v["daemons"].as_array().into_iter().flatten().filter_map(listed).collect())
    }

    /// A device's checks of a machine, before it opens a channel with its
    /// key: another account's machine against that account's root (pinned
    /// here on first sight), a hosted one against its project
    /// ([`accept_hosted`](Self::accept_hosted)). The certificate to open it
    /// with, or none for one of the account's own, which is checked against
    /// the account's devices when it's opened.
    fn check(&mut self, m: &Listed) -> anyhow::Result<Option<Cert>> {
        let mut changed = false;
        let checked = self.check_entry(m, &mut changed);
        if changed {
            self.save_device()?;
        }
        checked
    }

    fn check_entry(&mut self, m: &Listed, changed: &mut bool) -> anyhow::Result<Option<Cert>> {
        let d = &m.entry;
        if m.hosted {
            // A hosted machine (M178, M180): for its project only, as this
            // CLI's checks of the project say, never control's word.
            return self.accept_hosted(&d["hosted"], &mut None, changed).map(Some);
        }
        let Some(account) = m.account.clone() else { return Ok(None) };
        let chain =
            serde_json::from_value::<Chain>(d["chain"].clone()).context("control sent no certificates for it")?;
        let trust = chain.trust.filter(|t| t.account == account).context("control sent no root for its account")?;
        let owner = m.owner();
        let dev = self.dev_mut().context("not one of the account's devices")?;
        match dev.pins.get(&account) {
            Some(root) if *root != trust.root => bail!(
                "control says {owner}'s account is {} now, not {} as first seen here",
                fingerprint(&trust.root),
                fingerprint(root)
            ),
            Some(_) => {}
            None => {
                dev.pins.insert(account.clone(), trust.root.clone());
                *changed = true;
            }
        }
        let cert = trust.evaluate(&chain.certs, &chain.revocations).get(&m.id).cloned();
        cert.filter(|c| c.kind == Kind::Daemon).context("its certificate doesn't chain to its account").map(Some)
    }

    /// A hosted machine's certificate, made into the one its channel is
    /// opened with, if this CLI accepts it (`arugula_e2e::hosted::accept`):
    /// against its team's roster (that team's founder pinned here on first
    /// sight, as the page does) or this account's own devices. A device's
    /// only, for its key: a channel opened with a machine token never asks.
    fn accept_hosted(&mut self, v: &Value, teams: &mut Option<Vec<Value>>, pinned: &mut bool) -> anyhow::Result<Cert> {
        use arugula_e2e::{
            hosted::{Owner, Owners, accept_holding, ends_listed, read_cert, read_delegation},
            team::{AccountCerts, Roster, TeamPin},
        };
        let cert = read_cert(&v["cert"])?;
        let d = read_delegation(&v["delegation"])?;
        let (revokes, closed) = ends_listed(v);
        let (me, mut held, trust) = {
            let dev = self.dev().context("not one of the account's devices")?;
            (dev.cert.account.clone(), dev.closed.clone(), dev.trust.clone())
        };
        let ok = match d.owner().context("no owner")? {
            Owner::Team(id) => {
                if teams.is_none() {
                    let t = self.api("GET", "/api/teams", None)?;
                    *teams = Some(t["teams"].as_array().cloned().unwrap_or_default());
                }
                let t = teams.iter().flatten().find(|t| t["team"] == id.as_str()).context("not in its team")?;
                let pin: TeamPin = serde_json::from_value(t["pin"].clone())?;
                let roster: Roster = serde_json::from_value(t["roster"].clone())?;
                let certs: AccountCerts = serde_json::from_value(t["certs"].clone()).unwrap_or_default();
                let founder = format!("{}.{}", pin.founder, pin.founder_root);
                let key = format!("team:{id}");
                let pins = &mut self.dev_mut().context("not one of the account's devices")?.pins;
                match pins.get(&key) {
                    Some(f) if *f != founder => bail!("control says the team's founder is someone else now"),
                    Some(_) => {}
                    None => {
                        pins.insert(key, founder);
                        *pinned = true;
                    }
                }
                accept_holding(
                    &me,
                    &cert,
                    &d,
                    &revokes,
                    &closed,
                    &Owners::Team { roster: &roster, certs: &certs },
                    &mut held,
                )
            }
            Owner::Account(_) => {
                let v = self.api("GET", "/api/devices", None)?;
                let certs: Vec<Cert> = serde_json::from_value(v["certs"].clone()).unwrap_or_default();
                let revs: Vec<Revocation> = serde_json::from_value(v["revocations"].clone()).unwrap_or_default();
                accept_holding(
                    &me,
                    &cert,
                    &d,
                    &revokes,
                    &closed,
                    &Owners::Account { trust: &trust, certs: &certs, revocations: &revs },
                    &mut held,
                )
            }
        };
        let dev = self.dev_mut().context("not one of the account's devices")?;
        if held != dev.closed {
            dev.closed = held;
            *pinned = true;
        }
        ok.map_err(|r| anyhow::anyhow!("refused: {}", r.code()))?;
        // Its keys, to open a channel with; never one of the account's.
        Ok(Cert {
            v: 1,
            account: cert.project.clone(),
            device: cert.device.clone(),
            kind: Kind::Daemon,
            name: cert.name.clone(),
            noise: cert.noise.clone(),
            sign: cert.sign.clone(),
            created: cert.created,
            approver: String::new(),
            sig: String::new(),
        })
    }

    /// The account's devices that chain to the pinned root.
    fn trusted(&self, dev: &Device) -> anyhow::Result<arugula_e2e::cert::Trusted> {
        let v = self.api("GET", "/api/devices", None)?;
        let certs: Vec<Cert> = serde_json::from_value(v["certs"].clone()).unwrap_or_default();
        let revs: Vec<Revocation> = serde_json::from_value(v["revocations"].clone()).unwrap_or_default();
        Ok(dev.trust.evaluate(&certs, &revs))
    }

    /// The Noise key of one of the account's own machines, from the
    /// certificates control lists for it: for a directory from before
    /// control gave each machine's key (#994).
    fn own_noise(&self, id: &str) -> anyhow::Result<Option<String>> {
        let v = self.api("GET", "/api/devices", None)?;
        let certs: Vec<Cert> = serde_json::from_value(v["certs"].clone()).unwrap_or_default();
        Ok(certs.into_iter().find(|c| c.device == id && c.kind == Kind::Daemon).map(|c| c.noise))
    }

    /// A machine token for machine `id`, fresh: the daemon takes each once.
    fn machine_token(&self, id: &str) -> anyhow::Result<String> {
        let (status, body) = self.api_raw("POST", &token_path(id), Some(&json!({})))?;
        let v = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
        match v["token"].as_str() {
            Some(t) if status == 200 && !t.is_empty() => Ok(t.to_owned()),
            _ => {
                let said = v["error"].as_str().or(v["reason"].as_str()).map_or(format!("HTTP {status}"), str::to_owned);
                Err(NoToken { status, said }.into())
            }
        }
    }

    /// The relay's WebSocket for machine `id`, and how to ask for it.
    fn relay(&self, id: &str) -> anyhow::Result<(Url, String, Headers)> {
        let c = self.url()?;
        let path = format!("/api/relay/c/{id}");
        let ws = format!("{}://{}{path}", if c.tls { "wss" } else { "ws" }, c.authority);
        let mut headers = self.auth_headers("GET", &path, b"");
        // A cookie's WebSocket comes from control's own page; a signature
        // says it anyway.
        headers.retain(|(k, _)| *k != "Origin");
        headers.push(("origin", self.origin()));
        // A hosted machine that's asleep is woken (M182): the CLI opens a
        // machine only when someone asks it to.
        headers.push(("x-arugula-wake", "1".into()));
        Ok((c, ws, headers))
    }

    /// A channel to `machine`: with a machine token (aru-141), direct first,
    /// then through the relay. A device uses its key instead when tokens
    /// can't reach the machine ([`key_after_tokens`]).
    pub fn open(&mut self, machine: &Listed) -> anyhow::Result<Link> {
        let noise = match &machine.noise {
            Some(n) => Some(n.clone()),
            None => self.own_noise(&machine.id)?,
        };
        let tried = noise.as_deref().and_then(noise_key).map(|key| self.open_token(machine, key));
        let failed = match tried {
            Some(Ok(l)) => return Ok(l),
            Some(Err(e)) => Some(e),
            None => None,
        };
        if self.dev().is_none() {
            return Err(match failed {
                Some(e) => e.context(format!("can't reach {}", machine.name)),
                None => anyhow::anyhow!("control lists {}, but gives no key to reach it with", machine.name),
            });
        }
        if let Some(e) = failed {
            if !key_after_tokens(&e) {
                return Err(e.context(format!("can't reach {}", machine.name)));
            }
            if verbose() {
                eprintln!("arugula: {}: {e:#}; using this CLI's key", machine.name);
            }
        }
        self.open_device(machine)
    }

    /// [`open`](Self::open) with a machine token, each try with a new one.
    fn open_token(&self, machine: &Listed, noise: [u8; 32]) -> anyhow::Result<Link> {
        let daemon = Daemon { id: &machine.id, noise };
        let token = || self.machine_token(&machine.id);
        let mut errors = Vec::new();
        for u in &machine.urls {
            let Ok(url) = Url::parse(u) else { continue };
            let ws = format!("{}://{}/e2e", if url.tls { "wss" } else { "ws" }, url.authority);
            let route = format!("direct {u}, with a machine token");
            match Link::connect(&url, &ws, &[], &daemon, Hello::Token(&token), Some(DIRECT_TIMEOUT), route) {
                Ok(l) => return Ok(l),
                Err(e) if answered(&e) => return Err(e),
                Err(e) => errors.push(format!("{u}: {e:#}")),
            }
        }
        let (c, ws, headers) = self.relay(&machine.id)?;
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let route = "relayed through control, with a machine token".to_owned();
        match Link::connect(&c, &ws, &headers, &daemon, Hello::Token(&token), None, route) {
            Ok(l) => Ok(l),
            Err(e) if answered(&e) => Err(e),
            Err(e) => {
                errors.push(format!("the relay: {e:#}"));
                bail!("can't reach {} ({})", machine.name, errors.join("; "))
            }
        }
    }

    /// [`open`](Self::open) with this device's key, as before machine tokens.
    fn open_device(&mut self, machine: &Listed) -> anyhow::Result<Link> {
        let checked = self.check(machine).with_context(|| format!("not trusting {}", machine.name))?;
        let dev = self.dev().context("not one of the account's devices")?;
        let trusted = self.trusted(dev)?;
        if trusted.get(&dev.cert.device).is_none() {
            bail!(
                "this CLI isn't one of the account's devices any more (revoked?): `arugula logout`, then log in again"
            );
        }
        let cert = match checked {
            // Another account's or a hosted one, checked just now.
            Some(c) => c,
            None => trusted
                .get(&machine.id)
                .filter(|c| c.kind == Kind::Daemon)
                .with_context(|| format!("control lists {}, but it isn't a machine your account trusts", machine.name))?
                .clone(),
        };
        let daemon = Daemon { id: &cert.device, noise: cert.noise_key() };
        let mut errors = Vec::new();
        for u in &machine.urls {
            let Ok(url) = Url::parse(u) else { continue };
            let ws = format!("{}://{}/e2e", if url.tls { "wss" } else { "ws" }, url.authority);
            let hello = Hello::Key(&dev.keys);
            match Link::connect(&url, &ws, &[], &daemon, hello, Some(DIRECT_TIMEOUT), format!("direct {u}")) {
                Ok(l) => return Ok(l),
                Err(e) => errors.push(format!("{u}: {e:#}")),
            }
        }
        let (c, ws, headers) = self.relay(&machine.id)?;
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let hello = Hello::Key(&dev.keys);
        match Link::connect(&c, &ws, &headers, &daemon, hello, None, "relayed through control".into()) {
            Ok(l) => Ok(l),
            Err(e) => {
                errors.push(format!("the relay: {e:#}"));
                bail!("can't reach {} ({})", machine.name, errors.join("; "))
            }
        }
    }
}

/// A machine in control's directory.
#[derive(Deserialize, Clone, Debug)]
pub struct Listed {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub online: bool,
    /// Another account's machine (a team's, or shared): that account.
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub owner_name: Option<String>,
    /// The team it's in, if any.
    #[serde(default)]
    pub team: Option<String>,
    /// Its Noise key, hex, as control gives it (#994), else from the
    /// certificate listed beside it (a hosted machine's, or another
    /// account's chain): what a channel with a machine token is opened to.
    /// None for one of the account's own from an older control: its
    /// certificate in `/api/devices` says it.
    #[serde(default)]
    pub noise: Option<String>,
    /// A cloud workspace's machine (M178): through the relay only.
    #[serde(skip)]
    pub hosted: bool,
    /// The entry as control sent it, for a device's checks before it uses
    /// its key ([`Enrolled::check`]).
    #[serde(skip)]
    entry: Value,
}

impl Listed {
    /// Whose it is, for another account's machine.
    pub fn owner(&self) -> String {
        match (&self.owner_name, &self.account) {
            (Some(n), _) if !n.is_empty() => n.clone(),
            (_, Some(a)) => format!("account {}", &a[..a.len().min(8)]),
            _ => String::new(),
        }
    }
}

/// A directory entry. A hosted machine (M178) is reached only through the
/// relay: whatever the entry says, it has no direct URLs here.
fn listed(d: &Value) -> Option<Listed> {
    let mut m = serde_json::from_value::<Listed>(d.clone()).ok()?;
    let hosted = d.get("hosted").is_some_and(|h| !h.is_null());
    m.hosted = hosted;
    m.entry = d.clone();
    if hosted {
        m.urls.clear();
    }
    if m.noise.is_none() {
        // Control's word for its key, as the page takes it (`tokenEntry`).
        m.noise = if hosted {
            d["hosted"]["cert"]["noise"].as_str().map(str::to_owned)
        } else {
            d["chain"]["certs"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| c["device"] == m.id.as_str() && c["kind"] == "daemon")
                .and_then(|c| c["noise"].as_str())
                .map(str::to_owned)
        };
    }
    Some(m)
}

/// Another account's certificates, as the directory sends them.
#[derive(Deserialize)]
struct Chain {
    trust: Option<Trust>,
    #[serde(default)]
    certs: Vec<Cert>,
    #[serde(default)]
    revocations: Vec<Revocation>,
}

/// `--host NAME` through control: `None` when this CLI isn't logged in or
/// control doesn't list the name.
pub fn target(host: &str) -> anyhow::Result<Option<Target>> {
    let Some(mut e) = Enrolled::load()? else { return Ok(None) };
    let list = e.directory()?;
    let Some(m) = list.iter().find(|m| m.name == host).or_else(|| list.iter().find(|m| m.id == host)) else {
        return Ok(None);
    };
    let link = e.open(m)?;
    if verbose() {
        eprintln!("arugula: {}: {}", m.name, link.route);
    }
    Ok(Some(Target::Control(Arc::new(link))))
}

fn verbose() -> bool {
    std::env::var("ARUGULA_VERBOSE").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// `arugula hosts`' control part: the URL logged in to and its machines.
pub fn listing() -> anyhow::Result<Option<(String, Vec<Listed>)>> {
    let Some(mut e) = Enrolled::load()? else { return Ok(None) };
    let list = e.directory()?;
    Ok(Some((e.url.clone(), list)))
}

/// Whether this CLI is logged in to a control.
pub fn logged_in() -> bool {
    let dir = config_dir();
    dir.join(SESSION_FILE).exists() || dir.join(STATE_FILE).exists()
}

// ---------------------------------------------------------------- login

/// Where a browser that allowed the sign-in hands this CLI its grant, on
/// the loopback port it listens on (control's `app_login.rs`).
const LOOPBACK_PATH: &str = "/arugula-signin";

/// `arugula login --browser`: a session of the account at `url`, from a browser
/// that allows it (aru-141), the way the desktop app signs in: a verifier
/// this CLI keeps, a ticket from control for it, and the grant the browser
/// hands to this CLI's loopback port once someone signed in clicks Allow.
/// Only this CLI can swap the grant, since only it holds the verifier.
pub fn login(url: &str, name: &str) -> anyhow::Result<()> {
    let url = url.trim_end_matches('/').to_owned();
    match Enrolled::load()? {
        Some(e) if e.dev().is_some() => {
            bail!("this CLI is already logged in to {} as one of the account's devices (`arugula logout` first)", e.url)
        }
        // One whose session has ended is signed in again.
        Some(e) if e.api_raw("GET", "/api/me", None).is_ok_and(|(s, _)| s != 401) => {
            bail!("this CLI is already logged in to {} (`arugula logout` first)", e.url)
        }
        _ => {}
    }
    let s = sign_in(&url, name, |link, secs| {
        println!();
        println!("  To sign this terminal ({name}) in, open");
        println!();
        println!("    {link}");
        println!();
        println!("  in a browser that's signed in to Arugula cloud, and click Allow there.");
        println!("  If that browser is on another computer, it can't reach this one afterwards:");
        println!("  copy the address it shows then, and paste it here.");
        println!();
        println!("  Waiting (the link lasts {} minutes)…", secs / 60);
        let _ = std::io::stdout().flush();
        if std::io::stdout().is_terminal() {
            crate::util::open_in_browser(link);
        }
    })?;
    write_private(SESSION_FILE, &s)?;
    println!();
    let who = if s.login.is_empty() { String::new() } else { format!(" as {}", s.login) };
    println!("  Signed in{who}. `arugula hosts` lists your machines; `--host NAME` reaches one.");
    Ok(())
}

/// [`login`]'s exchange with control: a ticket, its link (`show` says it,
/// with how many seconds it lasts), the grant, and the session it's
/// swapped for, with the account it's of.
fn sign_in(url: &str, name: &str, show: impl FnOnce(&str, u64)) -> anyhow::Result<SavedSession> {
    use sha2::{Digest, Sha256};
    let u = Url::parse(url)?;
    if !u.tls && !private_host(&u.host) {
        bail!("control's URL must be https:// (or http on loopback or a private network, for testing)");
    }
    let verifier = hex(&arugula_e2e::random::<32>());
    let challenge = hex(&Sha256::digest(verifier.as_bytes()));
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").context("can't listen on this computer for the sign-in")?;
    let port = listener.local_addr()?.port();
    let ask = json!({ "name": name, "challenge": challenge, "port": port, "features": ["no-code"] }).to_string();
    let res = send_on(
        u.connect(Some(Duration::from_secs(10))).with_context(|| format!("can't reach control at {url}"))?,
        "POST",
        "/auth/app",
        &u.authority,
        &[("Content-Type", "application/json")],
        ask.as_bytes(),
    )?;
    let t =
        json_answer(res.status, res.bytes()?).with_context(|| format!("control at {url} didn't start a sign-in"))?;
    let ticket = t["ticket"].as_str().filter(|t| !t.is_empty() && t.bytes().all(|c| c.is_ascii_hexdigit()));
    let ticket = ticket.context("control's answer has no ticket")?.to_owned();
    let link = t["url"].as_str().filter(|l| l.starts_with("https://") || l.starts_with("http://"));
    let link = link.context("control's answer has no link")?;
    let secs = t["expires_in_secs"].as_u64().unwrap_or(600);
    show(link, secs);
    let grant = await_grant(&listener, &ticket, Instant::now() + Duration::from_secs(secs))?;
    let body = json!({ "grant": grant, "verifier": verifier }).to_string();
    let res = send_on(
        u.connect(Some(Duration::from_secs(10)))?,
        "POST",
        &format!("/auth/app/{ticket}/redeem"),
        &u.authority,
        // As control's own page sends it: the only place it takes one from.
        &[("Content-Type", "application/json"), ("Origin", url)],
        body.as_bytes(),
    )?;
    let set = res.header("set-cookie").and_then(|c| c.split(';').next()).map(str::trim).map(str::to_owned);
    let status = res.status;
    if !(200..300).contains(&status) {
        json_answer(status, res.bytes()?).context("control didn't sign this CLI in")?;
    }
    let cookie = set.filter(|c| c.contains('=')).context("control signed this CLI in, but sent no session")?;
    let e = Enrolled { url: url.to_owned(), account: String::new(), how: How::Session(cookie.clone()) };
    let me = e.api("GET", "/api/me", None).context("reading who this CLI is signed in as")?;
    let account = me["account"].as_str().filter(|a| !a.is_empty()).context("control didn't say which account")?;
    let login = me["login"].as_str().filter(|l| !l.is_empty()).or(me["name"].as_str()).unwrap_or_default();
    Ok(SavedSession { url: url.to_owned(), cookie, account: account.to_owned(), login: login.to_owned() })
}

/// The grant for `ticket`, when the browser that allowed it brings it to
/// the loopback port (`GET /arugula-signin?ticket=…&grant=…`), or when the
/// person pastes the address it went to; none past `deadline`.
fn await_grant(listener: &std::net::TcpListener, ticket: &str, deadline: Instant) -> anyhow::Result<String> {
    listener.set_nonblocking(true)?;
    let pasted = std::io::stdin().is_terminal().then(|| {
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        rx
    });
    loop {
        if Instant::now() > deadline {
            bail!("nobody allowed it in time; run `arugula login --browser` again for a new link");
        }
        if let Some(rx) = &pasted
            && let Ok(line) = rx.try_recv()
        {
            match grant_in(&line, ticket) {
                Some(g) => return Ok(g),
                None if line.trim().is_empty() => {}
                None => println!("  That isn't this sign-in's address: paste the whole address the browser went to."),
            }
        }
        let conn = match listener.accept() {
            Ok((c, _)) => c,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e).context("listening for the sign-in"),
        };
        let _ = conn.set_nonblocking(false);
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        if BufReader::new(&conn).read_line(&mut line).is_err() {
            continue;
        }
        let target = line.split_whitespace().nth(1).unwrap_or_default();
        let grant = target.starts_with(&format!("{LOOPBACK_PATH}?")).then(|| grant_in(target, ticket)).flatten();
        let reply = match &grant {
            Some(_) => {
                let page = "<!doctype html><meta charset=utf-8><title>Arugula CLI</title>\
                            <p style=\"font:16px system-ui;margin:3em\">Signed in. You can close this tab and go back to the terminal.</p>";
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                )
            }
            None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
        };
        let _ = (&conn).write_all(reply.as_bytes());
        if let Some(g) = grant {
            return Ok(g);
        }
    }
}

/// The grant in an address the browser was sent to for `ticket`: the
/// loopback request's target, or the whole address as pasted.
fn grant_in(address: &str, ticket: &str) -> Option<String> {
    let query = address.trim().split_once('?')?.1;
    let query = query.split(['#', ' ']).next().unwrap_or_default();
    let param = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='));
    if param("ticket")? != ticket {
        return None;
    }
    let grant = param("grant")?;
    (!grant.is_empty() && grant.len() <= 256 && grant.bytes().all(|c| c.is_ascii_hexdigit())).then(|| grant.to_owned())
}

/// `arugula login` (the default; `--device` is kept as a hidden no-op): join the account at `url` as a `cli` device,
/// as before machine tokens (aru-141; it goes with the key code, #1007).
/// Shows the link to approve on a signed-in device and waits; then checks the
/// account before pinning it. The link carries a join secret (#760) when
/// control passes its MAC on: the approving device's MAC under it says
/// which account, with nothing to compare. Otherwise (an older control)
/// `account`, the fingerprint the approving device shows, or asking.
pub fn login_device(url: &str, name: &str, account: Option<&str>) -> anyhow::Result<()> {
    let url = url.trim_end_matches('/').to_owned();
    if let Some(e) = Enrolled::load()? {
        bail!("this CLI is already logged in to {} (`arugula logout` first)", e.url);
    }
    let want = account.map(parse_fingerprint).transpose()?;
    let u = Url::parse(&url)?;
    if !u.tls && !private_host(&u.host) {
        bail!("control's URL must be https:// (or http on loopback or a private network, for testing)");
    }
    let get = |path: &str| -> anyhow::Result<crate::http::Response> {
        send_on(
            u.connect(Some(Duration::from_secs(10)))?,
            "GET",
            path,
            &u.authority,
            &[(wire::FEATURES_HEADER, wire::APP_DEVICES)],
            b"",
        )
    };
    let about =
        get(wire::CONTROL_JSON).and_then(|r| r.json()).with_context(|| format!("can't reach control at {url}"))?;
    // Only this field, so one the CLI never reads can't make control look older.
    let takes_cli = serde_json::from_value::<wire::ControlCliJoin>(about.clone()).ok().and_then(|c| c.cli_join);
    // A secret in the link (#760), never sent to control.
    let secret = serde_json::from_value::<wire::ControlJoinSecret>(about)
        .is_ok_and(|c| c.join_secret >= 1)
        .then(arugula_e2e::cert::join_secret);
    if takes_cli.is_none() {
        bail!("control at {url} doesn't take the Arugula CLI as a device yet (it's older than this CLI)");
    }
    let keys = DeviceKeys::generate();
    let ask = Cert { account: String::new(), ..Cert::new(&keys, "", Kind::Cli, name) };
    let ms = now_ms();
    let proof = wire::JoinProof { ms, sig: hex(&keys.signature(join_proof_body(&ask, ms).as_bytes())) };
    let features = if secret.is_some() { arugula_e2e::cert::JOIN_SECRET.to_owned() } else { String::new() };
    let body = serde_json::to_string(&wire::CliJoinRequest { cert: ask.clone(), proof, features })?;
    let started = send_on(
        u.connect(Some(Duration::from_secs(10)))?,
        "POST",
        wire::JOIN,
        &u.authority,
        &[("Content-Type", "application/json")],
        body.as_bytes(),
    )?
    .json()
    .context("control")?;
    let started: wire::JoinStarted =
        serde_json::from_value(started).context("control sent no code or poll token it could be joined with")?;
    let (code, poll, secs) = (started.code, started.poll, started.expires_in_secs);
    if code != arugula_e2e::cert::join_code(&ask) {
        bail!("control sent a code that isn't this key's; not logging in");
    }
    let whole = match &secret {
        Some(s) => format!("{code}.{s}"),
        None => code.clone(),
    };
    println!();
    println!("  To make this terminal ({name}) one of your devices, open");
    println!();
    println!("    {url}/#join={whole}");
    println!();
    if secret.is_some() {
        println!("  on a device that's signed in, and approve it there.");
        println!("  Or sign in at {url} and type the whole code, {whole}.");
    } else {
        println!("  on a device that's signed in, and check the code there is {code}.");
        println!("  Or sign in at {url} and type the code.");
    }
    println!();
    println!("  Waiting for approval (the code lasts {} minutes)…", secs / 60);
    std::io::stdout().flush()?;
    let deadline = Instant::now() + Duration::from_secs(secs);
    let got = loop {
        if Instant::now() > deadline {
            bail!("nobody approved it in {} minutes; run `arugula login` again for a new code", secs / 60);
        }
        std::thread::sleep(Duration::from_secs(2));
        let Ok(res) = get(&wire::JoinPoll::path(&code, &poll)) else { continue };
        if res.status == 404 {
            bail!("the code expired; run `arugula login` again");
        }
        let v: wire::JoinPoll = serde_json::from_value(res.json().context("control")?)
            .context("control's answer about the approval isn't one this CLI can read")?;
        if let Some(on) = &v.rejected {
            bail!("turned down on {on}");
        }
        if v.approved {
            break v;
        }
    };
    let cert = got.cert.context("control approved it but sent no certificate")?;
    let trust = got.trust.context("control sent no account root")?;
    let (certs, revs) = (got.certs, got.revocations);
    if !cert.same_request(&ask) {
        bail!("control sent back a certificate for a different key; not logging in");
    }
    // Which check failed, if one did (#327).
    if let Some(r) = trust.refusal(&certs, &revs, &cert) {
        bail!("the approval doesn't check out against the account's devices: {}; not logging in", r.check());
    }
    let approver = trust.evaluate(&certs, &revs).get(&cert.approver).map(|c| c.name.clone()).unwrap_or_default();
    let fp = fingerprint(&trust.root);
    // The approving device's MAC under the link's secret (#760): one guess
    // per code, and this key is never saved unless it checks out.
    let proven = match (&secret, &got.mac) {
        (Some(s), Some(mac)) if arugula_e2e::cert::join_mac_ok(s, &trust, &cert, mac) => true,
        (Some(_), Some(_)) => bail!(
            "the approval doesn't match the secret in this terminal's link: the code was mistyped, or control changed the account it's approved into. Not logging in; run `arugula login` again for a new code (and remove this one under Devices and machines… if it's listed)"
        ),
        _ => false,
    };
    match want {
        Some(w) if w != trust.root => {
            bail!("control approved this CLI into the account {fp}, not {}; not logging in", fingerprint(&w))
        }
        Some(_) => {}
        None if proven => {}
        None if secret.is_some() => bail!(
            "the approval came without the approving device's answer to this terminal's secret (approved from the code alone?), so control's word is all there is for the account; not logging in. Open the whole link, or type the whole code, or pass --account with the fingerprint your device shows"
        ),
        None => confirm(&approver, &fp, &trust.root)?,
    }
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    keys.save(&dir.join(KEY_FILE))?;
    Saved { url: url.clone(), cert, trust, pins: BTreeMap::new(), closed: Default::default() }.write()?;
    println!();
    println!("  Logged in. This terminal is one of your devices, approved on \"{approver}\".");
    println!("  The account is {fp}. `arugula hosts` lists your machines; `--host NAME` reaches one.");
    Ok(())
}

fn confirm(approver: &str, fp: &str, root: &str) -> anyhow::Result<()> {
    println!();
    println!("  Approved on \"{approver}\". Before this CLI trusts it, check the account:");
    println!();
    println!("    {fp}");
    println!();
    println!(
        "  The device you approved on shows its account's fingerprint under Devices and machines… in the account menu."
    );
    print!("  Is it the same? [y/N, or type the fingerprint] ");
    std::io::stdout().flush()?;
    if !std::io::stdin().is_terminal() {
        println!();
        bail!("no terminal to ask in, so not logging in; pass --account with the fingerprint your device shows");
    }
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let said = line.trim();
    if matches!(said.to_ascii_lowercase().as_str(), "y" | "yes") || parse_fingerprint(said).is_ok_and(|f| f == root) {
        return Ok(());
    }
    bail!("not logging in. If the fingerprints differ, control isn't telling the truth about your account")
}

/// `arugula logout`: end the session (control too, if it's reached), or
/// forget a device's key and pin. A device stays on the account's list
/// until a device revokes it.
pub fn logout() -> anyhow::Result<()> {
    let dir = config_dir();
    let Some(e) = Enrolled::load()? else {
        println!("Not logged in.");
        return Ok(());
    };
    let Some(dev) = e.dev() else {
        // Best effort: the file goes whatever control says.
        let _ = e.api_raw("POST", "/auth/logout", None);
        fs::remove_file(dir.join(SESSION_FILE))?;
        println!("Logged out of {}.", e.url);
        return Ok(());
    };
    fs::remove_file(dir.join(STATE_FILE))?;
    fs::remove_file(dir.join(KEY_FILE))?;
    println!(
        "Logged out of {}. This terminal ({}) is still listed on the account: remove it under Devices and machines… to revoke it.",
        e.url, dev.cert.name
    );
    Ok(())
}

/// A fingerprint as typed: 16 hex digits, dashes optional.
fn parse_fingerprint(typed: &str) -> anyhow::Result<String> {
    let id: String = typed.chars().filter(|c| *c != '-').map(|c| c.to_ascii_lowercase()).collect();
    if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("an account fingerprint is 16 hex digits, like 1a2b-3c4d-5e6f-7a8b");
    }
    Ok(id)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Loopback or a private address (or `localhost`): plain http is allowed
/// there, for tests and labs.
fn private_host(host: &str) -> bool {
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => v4.is_loopback() || v4.is_private(),
        Ok(std::net::IpAddr::V6(v6)) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        Err(_) => host == "localhost",
    }
}

// ---------------------------------------------------------------- the channel

/// One part of an answer: (status, content type, body, more to come).
type Part = (u16, Option<String>, Vec<u8>, bool, std::collections::BTreeMap<String, String>);
type Reply = mpsc::Sender<Part>;

/// What the socket's thread is given.
enum Out {
    /// A message to seal and send; a request's answer goes to `Reply`.
    Send(Msg, Option<Reply>),
    /// Where the daemon's protocol messages go from now on (the `/ws` of
    /// this command), and how to wake whoever reads them.
    Sink(mpsc::Sender<Msg>, crate::wake::Waker),
}

/// What the daemon sends before a `/ws` is there to take it (its hello,
/// first of all) is kept, up to this much.
const EARLY_MAX: usize = 64 << 20;

/// One Noise channel to a machine, shared by every request a command makes.
/// A thread owns the socket: it seals what's queued, in order, and hands
/// each response to the request waiting for it.
pub struct Link {
    out: Mutex<mpsc::Sender<Out>>,
    waker: crate::wake::Waker,
    next: AtomicU32,
    /// How it got there, in words.
    pub route: String,
}

/// The machine a channel is opened to: its id (the prologue's) and its
/// Noise key.
struct Daemon<'a> {
    id: &'a str,
    noise: [u8; 32],
}

/// What a channel's first message says about who's asking.
enum Hello<'a> {
    /// A device of the account, by its key (M49; #1007 removes it).
    Key(&'a DeviceKeys),
    /// Control's word (aru-141): a fresh machine token from this.
    Token(&'a dyn Fn() -> anyhow::Result<String>),
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Link({})", self.route)
    }
}

impl Link {
    fn connect(
        at: &Url,
        ws_url: &str,
        headers: &[(&str, &str)],
        daemon: &Daemon,
        hello: Hello,
        connect_timeout: Option<Duration>,
        route: String,
    ) -> anyhow::Result<Self> {
        let token = matches!(hello, Hello::Token(_));
        let why = if token { "" } else { " (not an approved device?)" };
        use tungstenite::client::IntoClientRequest;
        let stream = at.connect(connect_timeout.or(Some(Duration::from_secs(10))))?;
        stream.set_timeout(Some(HANDSHAKE_TIMEOUT))?;
        let mut req = ws_url.into_client_request()?;
        for (k, v) in headers {
            req.headers_mut().insert(tungstenite::http::HeaderName::from_bytes(k.as_bytes())?, v.parse()?);
        }
        let (mut ws, _) = tungstenite::client(req, stream).map_err(|e| match e {
            tungstenite::HandshakeError::Failure(tungstenite::Error::Http(r)) => {
                let body = r.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default();
                let said = serde_json::from_str::<Value>(&body)
                    .ok()
                    .and_then(|v| v["error"].as_str().map(String::from))
                    .unwrap_or_else(|| body.to_string());
                anyhow::anyhow!("{} {said}", r.status())
            }
            tungstenite::HandshakeError::Failure(e) => anyhow::Error::from(e).context("websocket handshake"),
            tungstenite::HandshakeError::Interrupted(_) => anyhow::anyhow!("websocket handshake interrupted"),
        })?;
        // Noise IK, the CLI the initiator: with this device's key, or with
        // a key made for this channel alone and a machine token (aru-141),
        // asked for only now that the socket is there, since the daemon
        // takes each token once.
        let (init, m1) = match hello {
            Hello::Key(keys) => Initiator::start(keys, &daemon.noise, &prologue(daemon.id))?,
            Hello::Token(next) => {
                let hello = TokenHello { machine_token: next()? }.encode();
                Initiator::start_with(&DeviceKeys::generate(), &daemon.noise, &prologue(daemon.id), &hello)?
            }
        };
        ws.send(Message::Binary(m1.into()))?;
        let m2 = loop {
            match ws.read() {
                Ok(Message::Binary(b)) => break b,
                Ok(Message::Close(_)) => bail!("closed during the handshake{why}"),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    bail!("no answer to the handshake")
                }
                Err(e) => {
                    return Err(anyhow::Error::from(e).context(format!("closed during the handshake{why}")));
                }
            }
        };
        let (said, ch) = init.finish(&m2)?;
        if let Some(r) = Refusal::decode(&said) {
            if token {
                return Err(Refused { code: r.refused, message: r.message }.into());
            }
            bail!("the machine refused: {}{why}", r.message);
        }
        ws.get_ref().set_timeout(None)?;
        ws.get_mut().set_nonblocking(true)?;
        let (tx, rx) = mpsc::channel();
        let (wake, waker) = crate::wake::Wake::pair()?;
        std::thread::spawn(move || io(ws, ch, rx, wake));
        Ok(Self { out: Mutex::new(tx), waker, next: AtomicU32::new(1), route })
    }

    fn put(&self, o: Out) -> anyhow::Result<()> {
        self.out.lock().unwrap().send(o).map_err(|_| anyhow::anyhow!("the channel closed"))?;
        self.waker.wake();
        Ok(())
    }

    /// One request; its answer comes in one part, or in several when
    /// `stream` is set and the daemon has no end to it.
    fn request(
        &self,
        method: &str,
        path: &str,
        content_type: Option<String>,
        body: Vec<u8>,
        stream: bool,
    ) -> anyhow::Result<mpsc::Receiver<Part>> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        let head =
            RequestHead { method: method.into(), path: path.into(), content_type, stream, headers: Default::default() };
        self.put(Out::Send(Msg::Request { id, head, body }, Some(tx)))?;
        Ok(rx)
    }

    /// A connection the HTTP client can use as if it were a socket to the
    /// daemon: each request on it is carried over the channel, and a
    /// WebSocket to `/ws` is the channel's protocol messages.
    pub fn stream(self: &Arc<Self>) -> anyhow::Result<UnixStream> {
        let (ours, theirs) = pair()?;
        let link = self.clone();
        std::thread::spawn(move || {
            let _ = serve_one(&link, theirs);
        });
        Ok(ours)
    }
}

/// Two connected ends, for a request carried in this process.
#[cfg(unix)]
fn pair() -> std::io::Result<(UnixStream, UnixStream)> {
    UnixStream::pair()
}

/// Windows: a loopback TCP pair. Whoever connects first to the listener is
/// checked to be us (by its address) before it's used.
#[cfg(not(unix))]
fn pair() -> std::io::Result<(UnixStream, UnixStream)> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    let ours = std::net::TcpStream::connect(l.local_addr()?)?;
    loop {
        let (theirs, from) = l.accept()?;
        if from == ours.local_addr()? {
            ours.set_nodelay(true)?;
            theirs.set_nodelay(true)?;
            return Ok((ours, theirs));
        }
    }
}

/// Read one HTTP/1.1 request from `s`, carry it, write its answer.
fn serve_one(link: &Link, s: UnixStream) -> anyhow::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut words = line.split_whitespace();
    let (method, path) = (words.next().unwrap_or("GET").to_owned(), words.next().unwrap_or("/").to_owned());
    let (mut len, mut ct, mut upgrade, mut key) = (0usize, None, false, None);
    loop {
        line.clear();
        r.read_line(&mut line)?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':').unwrap_or((l, ""));
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.trim().parse().unwrap_or(0),
            "content-type" => ct = Some(v.trim().to_owned()),
            "upgrade" => upgrade = v.trim().eq_ignore_ascii_case("websocket"),
            "sec-websocket-key" => key = Some(v.trim().to_owned()),
            _ => {}
        }
    }
    let mut w = s;
    if upgrade {
        let early = r.buffer().to_vec();
        return match key {
            Some(key) if path == "/ws" => serve_ws(link, w, early, &key),
            _ => {
                let e = json!({"error": "only /ws goes through control as a WebSocket"}).to_string();
                write_head(&mut w, 404, Some("application/json"), &Default::default(), Some(e.len()))?;
                w.write_all(e.as_bytes())?;
                Ok(())
            }
        };
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let failed = |w: &mut UnixStream, e: String| -> anyhow::Result<()> {
        let e = json!({"error": e}).to_string();
        write_head(w, 502, Some("application/json"), &Default::default(), Some(e.len()))?;
        w.write_all(e.as_bytes())?;
        Ok(())
    };
    let parts = match link.request(&method, &path, ct, body, true) {
        Ok(p) => p,
        Err(e) => return failed(&mut w, format!("{e:#}")),
    };
    let Ok((status, ct, body, more, headers)) = parts.recv() else {
        return failed(&mut w, "the channel to the machine closed".into());
    };
    if !more {
        write_head(&mut w, status, ct.as_deref(), &headers, Some(body.len()))?;
        w.write_all(&body)?;
        w.flush()?;
        return Ok(());
    }
    // In parts as they come, chunked; a closed channel ends it unfinished.
    write_head(&mut w, status, ct.as_deref(), &headers, None)?;
    let chunk = |w: &mut UnixStream, b: &[u8]| -> std::io::Result<()> {
        if !b.is_empty() {
            write!(w, "{:x}\r\n", b.len())?;
            w.write_all(b)?;
            w.write_all(b"\r\n")?;
            w.flush()?;
        }
        Ok(())
    };
    chunk(&mut w, &body)?;
    while let Ok((_, _, body, more, _)) = parts.recv() {
        chunk(&mut w, &body)?;
        if !more {
            w.write_all(b"0\r\n\r\n")?;
            break;
        }
    }
    w.flush()?;
    Ok(())
}

/// A response head; chunked without a length.
fn write_head(
    w: &mut UnixStream,
    status: u16,
    ct: Option<&str>,
    headers: &std::collections::BTreeMap<String, String>,
    len: Option<usize>,
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} X\r\nConnection: close\r\n");
    match len {
        Some(n) => head.push_str(&format!("Content-Length: {n}\r\n")),
        None => head.push_str("Transfer-Encoding: chunked\r\n"),
    }
    if let Some(ct) = ct {
        head.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    // The machine's words, so nothing that would end a line.
    for (k, v) in headers.iter().filter(|(k, v)| !k.contains(['\r', '\n', ':']) && !v.contains(['\r', '\n'])) {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    w.write_all(head.as_bytes())
}

/// `/ws` over the channel: the WebSocket's messages are the channel's
/// `T`/`B` messages, both ways, until either end closes.
fn serve_ws(link: &Link, mut s: UnixStream, early: Vec<u8>, key: &str) -> anyhow::Result<()> {
    use tungstenite::protocol::Role;
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    write!(
        s,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    let (tx, rx) = mpsc::channel();
    let (mut wake, waker) = crate::wake::Wake::pair()?;
    link.put(Out::Sink(tx, waker))?;
    s.set_nonblocking(true)?;
    let mut ws = WebSocket::from_partially_read(s, early, Role::Server, None);
    loop {
        let _ = wake.wait(Some(ws.get_ref()), Duration::from_millis(50));
        wake.drain();
        loop {
            let m = match rx.try_recv() {
                Ok(Msg::Text(t)) => Message::Text(t.into()),
                Ok(Msg::Binary(b)) => Message::Binary(b.into()),
                Ok(_) => continue,
                Err(mpsc::TryRecvError::Empty) => break,
                // The channel ended.
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Ok(());
                }
            };
            match ws.write(m) {
                Ok(()) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => return Ok(()),
            }
        }
        match ws.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => return Ok(()),
        }
        loop {
            match ws.read() {
                Ok(Message::Text(t)) => link.put(Out::Send(Msg::Text(t.to_string()), None))?,
                Ok(Message::Binary(b)) => link.put(Out::Send(Msg::Binary(b.to_vec()), None))?,
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => return Ok(()),
            }
        }
    }
}

/// Where the daemon's protocol messages go.
enum Sink {
    /// No `/ws` yet: kept for it (`None` once there was too much).
    Early(Option<Vec<Msg>>, usize),
    To(mpsc::Sender<Msg>, crate::wake::Waker),
    /// Its `/ws` closed; the channel is one client, so there's no other.
    Gone,
}

impl Sink {
    fn take(&mut self, m: Msg) {
        match self {
            Sink::Early(Some(kept), n) => {
                *n += match &m {
                    Msg::Text(t) => t.len(),
                    Msg::Binary(b) => b.len(),
                    _ => 0,
                };
                kept.push(m);
                if *n > EARLY_MAX {
                    *self = Sink::Early(None, 0);
                }
            }
            Sink::Early(None, _) | Sink::Gone => {}
            Sink::To(tx, wake) => {
                if tx.send(m).is_ok() {
                    wake.wake();
                } else {
                    *self = Sink::Gone;
                }
            }
        }
    }

    /// The `/ws` arrived: what was kept goes first. A second `/ws`, or one
    /// after too much went unread, gets nothing and closes.
    fn attach(&mut self, tx: mpsc::Sender<Msg>, wake: crate::wake::Waker) {
        let Sink::Early(Some(kept), _) = std::mem::replace(self, Sink::Gone) else { return };
        for m in kept {
            let _ = tx.send(m);
        }
        wake.wake();
        *self = Sink::To(tx, wake);
    }
}

/// The socket's thread: until every `Link` handle is gone or the socket
/// closes.
fn io(mut ws: WebSocket<Box<dyn Stream>>, ch: Channel, rx: mpsc::Receiver<Out>, mut wake: crate::wake::Wake) {
    let mut waiting: HashMap<u32, Reply> = HashMap::new();
    let mut sink = Sink::Early(Some(Vec::new()), 0);
    'outer: loop {
        let _ = wake.wait(Some(ws.get_ref().as_ref()), Duration::from_millis(100));
        wake.drain();
        loop {
            match rx.try_recv() {
                Ok(Out::Sink(tx, wake)) => sink.attach(tx, wake),
                Ok(Out::Send(m, reply)) => {
                    if let (Msg::Request { id, .. }, Some(r)) = (&m, reply) {
                        waiting.insert(*id, r);
                    }
                    let Ok(wires) = ch.seal(&m) else { break 'outer };
                    for w in wires {
                        match ws.write(Message::Binary(w.into())) {
                            Ok(()) => {}
                            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                            Err(_) => break 'outer,
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break 'outer,
            }
        }
        match ws.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        // Always read until there's nothing: TLS or tungstenite may hold a
        // whole message the poll above didn't see.
        loop {
            match ws.read() {
                Ok(Message::Binary(w)) => match ch.open(&w) {
                    Ok(Some(Msg::Response { id, head, body })) => {
                        let more = head.more;
                        let gone = match waiting.get(&id) {
                            Some(r) => r.send((head.status, head.content_type, body, more, head.headers)).is_err(),
                            None => false,
                        };
                        if !more || gone {
                            waiting.remove(&id);
                        }
                    }
                    Ok(Some(m @ (Msg::Text(_) | Msg::Binary(_)))) => sink.take(m),
                    Ok(_) => {}
                    Err(_) => break 'outer,
                },
                Ok(Message::Close(_)) => break 'outer,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break 'outer,
            }
        }
    }
    let _ = ws.close(None);
    let _ = ws.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints() {
        assert_eq!(parse_fingerprint("1A2B-3c4d-5e6f-7a8b").unwrap(), "1a2b3c4d5e6f7a8b");
        assert!(parse_fingerprint("1a2b").is_err());
        assert!(parse_fingerprint("zzzz-3c4d-5e6f-7a8b").is_err());
    }

    /// M141: control's maintenance 503 is waited out, even for a POST, and
    /// each try is signed afresh; another 503 to a POST isn't sent again.
    #[test]
    fn a_maintenance_window_is_waited_out() {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let answers = ["503 Service Unavailable\r\nRetry-After: 0", "200 OK", "503 Service Unavailable"];
        let bodies = [r#"{"error":"down for maintenance","reason":"maintenance"}"#, r#"{"grants":[]}"#, "no upstream"];
        let server = std::thread::spawn(move || {
            let mut auths = Vec::new();
            for (head, body) in answers.iter().zip(bodies) {
                let (sock, _) = l.accept().unwrap();
                let mut r = BufReader::new(sock.try_clone().unwrap());
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if let Some(v) = lower.strip_prefix("x-arugula-auth:") {
                        auths.push(v.trim().to_owned());
                    }
                    if line.trim().is_empty() {
                        break;
                    }
                }
                r.read_exact(&mut vec![0; len]).unwrap();
                let mut sock = sock;
                write!(sock, "HTTP/1.1 {head}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            }
            auths
        });
        let e = Enrolled::for_tests(url);
        let restore = json!({ "daemon": "d1", "power": "cloud.drive" });
        assert_eq!(e.api("POST", "/api/grants", Some(&restore)).unwrap(), json!({ "grants": [] }));
        let err = e.api("POST", "/api/grants", Some(&restore)).unwrap_err();
        assert!(format!("{err:#}").contains("no upstream"), "{err:#}");
        let auths = server.join().unwrap();
        assert_eq!(auths.len(), 3, "the plain 503 asked once");
        assert_ne!(auths[0], auths[1], "signed afresh");
    }

    /// A hosted machine is reached only through the relay: the CLI keeps
    /// no direct URL for it, whatever the directory says; a joined machine
    /// keeps its own.
    #[test]
    fn a_hosted_machine_has_no_direct_urls() {
        let joined = json!({ "id": "d1", "name": "geek", "urls": ["http://geek:7681"] });
        let m = listed(&joined).unwrap();
        assert_eq!((m.urls, m.hosted), (vec!["http://geek:7681".to_owned()], false));
        let hosted = json!({
            "id": "h1", "name": "hosted box", "urls": ["http://10.0.0.5:7681"],
            "hosted": { "cert": {}, "delegation": {}, "revocations": [] },
        });
        let m = listed(&hosted).unwrap();
        assert!(m.hosted);
        assert!(m.urls.is_empty(), "{:?}", m.urls);
        let m = listed(&json!({ "id": "d2", "name": "box", "urls": ["http://box:7681"], "hosted": null })).unwrap();
        assert!(!m.hosted && m.urls.len() == 1);
    }

    /// This CLI keeps control's word that a delegation ended: a machine it
    /// refused stays refused when control leaves the word out, and the word
    /// is in what it saves.
    #[test]
    fn the_cli_keeps_a_delegations_end() {
        use arugula_e2e::hosted::{Delegation, HostedCert, HostedKey};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let keys = DeviceKeys::generate();
        let mut me = Cert::new(&keys, "a", Kind::Browser, "laptop");
        me.sign_with(&keys);
        let devices = json!({ "certs": [me], "revocations": [] }).to_string();
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            for sock in listener.incoming().take(3) {
                let mut r = BufReader::new(sock.unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    r.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                let mut sock = r.into_inner();
                write!(
                    sock,
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{devices}",
                    devices.len()
                )
                .unwrap();
            }
        });
        let key = HostedKey::from_seed(&[9; 32]);
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: "account:a".into(),
            key: key.public(),
            at: 1,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&keys);
        let boxk = DeviceKeys::generate();
        let as_cert = Cert::new(&boxk, "a", Kind::Daemon, "box");
        let mut cert = HostedCert::new(&d, "hw_1", "box", &as_cert.noise, &as_cert.sign, 40);
        key.sign(&mut cert);
        let mut e = Enrolled::device(
            url,
            Device {
                cert: me.clone(),
                trust: Trust { account: "a".into(), root: me.device.clone() },
                pins: BTreeMap::new(),
                closed: Default::default(),
                keys,
            },
        );
        let plain = json!({ "cert": cert, "delegation": d, "revocations": [] });
        let told = json!({ "cert": cert, "delegation": d, "revocations": [], "closed": [key.close(&d, 30), {}] });
        let (mut teams, mut changed) = (None, false);
        assert!(e.accept_hosted(&plain, &mut teams, &mut changed).is_ok(), "it's heard nothing yet");
        assert!(!changed);
        assert!(e.accept_hosted(&told, &mut teams, &mut changed).is_err());
        assert!(changed, "there's something to save");
        assert!(e.accept_hosted(&plain, &mut teams, &mut changed).is_err(), "control left it out");
        server.join().unwrap();
        let dev = e.dev().unwrap();
        let saved = Saved {
            url: e.url.clone(),
            cert: dev.cert.clone(),
            trust: dev.trust.clone(),
            pins: BTreeMap::new(),
            closed: dev.closed.clone(),
        };
        let back: Saved = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(back.closed, dev.closed);
    }

    #[test]
    fn plain_http_only_nearby() {
        for ok in ["127.0.0.1", "10.229.85.10", "192.168.1.2", "localhost", "::1"] {
            assert!(private_host(ok), "{ok}");
        }
        for no in ["control.arugula.io", "8.8.8.8"] {
            assert!(!private_host(no), "{no}");
        }
    }

    /// One HTTP request as a test's control reads it.
    struct Asked {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Asked {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
        }
    }

    /// A control of the test's own on `l`: each request in turn, answered by
    /// `answer` with a status, extra header lines and a body, until it says
    /// it's done (`None`).
    fn control(
        l: std::net::TcpListener,
        mut answer: impl FnMut(&Asked) -> Option<(u16, String, String)> + Send + 'static,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            for sock in l.incoming() {
                let sock = sock.unwrap();
                let mut r = BufReader::new(sock.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut words = line.split_whitespace();
                let (method, path) = (words.next().unwrap_or("").to_owned(), words.next().unwrap_or("").to_owned());
                let mut headers = Vec::new();
                loop {
                    line.clear();
                    r.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    let (k, v) = line.split_once(':').unwrap();
                    headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
                }
                let len = headers.iter().find(|(k, _)| k == "content-length").map_or(0, |(_, v)| v.parse().unwrap());
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let Some((status, extra, body)) = answer(&Asked { method, path, headers, body }) else { return };
                let mut sock = sock;
                write!(
                    sock,
                    "HTTP/1.1 {status} X\r\n{extra}Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        })
    }

    /// `arugula login` (aru-141): control's link, a browser's Allow that
    /// hands the grant to this CLI's loopback port (one for another sign-in
    /// is turned away first), and a session for the verifier only this CLI
    /// holds, swapped from control's own origin; then whose session it is.
    #[test]
    fn a_cli_signs_in_from_a_browser_with_its_own_verifier() {
        use sha2::{Digest, Sha256};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let at = url.clone();
        let ticket = "ab".repeat(16);
        let challenge = Arc::new(Mutex::new(String::new()));
        let server = control(l, move |a| {
            Some(match (a.method.as_str(), a.path.as_str()) {
                ("POST", "/auth/app") => {
                    let ask: Value = serde_json::from_slice(&a.body).unwrap();
                    assert_eq!(ask["name"], "Arugula CLI on test");
                    assert_eq!(ask["features"], json!(["no-code"]));
                    assert!(ask.get("device").is_none(), "no device keys: it isn't a device");
                    *challenge.lock().unwrap() = ask["challenge"].as_str().unwrap().to_owned();
                    let port = ask["port"].as_u64().unwrap();
                    // The browser, once someone clicks Allow.
                    let mine = ticket.clone();
                    std::thread::spawn(move || {
                        let hand = |t: &str| {
                            let mut s = std::net::TcpStream::connect(("127.0.0.1", port as u16)).unwrap();
                            write!(s, "GET /arugula-signin?ticket={t}&grant=cafe HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                            let mut got = String::new();
                            let _ = s.read_to_string(&mut got);
                            got
                        };
                        assert!(hand(&"cd".repeat(16)).starts_with("HTTP/1.1 404"), "another sign-in's grant");
                        assert!(hand(&mine).contains("Signed in."));
                    });
                    let link = format!("{at}/#app={ticket}");
                    (200, String::new(), json!({ "ticket": ticket, "url": link, "expires_in_secs": 60 }).to_string())
                }
                ("POST", p) if p == format!("/auth/app/{ticket}/redeem") => {
                    assert_eq!(a.header("origin"), Some(at.as_str()), "from control's own origin");
                    let b: Value = serde_json::from_slice(&a.body).unwrap();
                    assert_eq!(b["grant"], "cafe");
                    let v = b["verifier"].as_str().unwrap();
                    let sha: String = Sha256::digest(v.as_bytes()).iter().map(|x| format!("{x:02x}")).collect();
                    assert_eq!(sha, *challenge.lock().unwrap(), "the verifier the challenge was made from");
                    (204, "Set-Cookie: ilg_session=s3cret; Path=/; HttpOnly\r\n".into(), String::new())
                }
                ("GET", "/api/me") => {
                    assert_eq!(a.header("cookie"), Some("ilg_session=s3cret"));
                    assert!(a.header("x-arugula-auth").is_none());
                    let me = json!({ "account": "acc_me", "login": "me", "name": "Me" });
                    (200, String::new(), me.to_string())
                }
                other => panic!("unexpected {other:?}"),
            })
        });
        let mut shown = None;
        let s = sign_in(&url, "Arugula CLI on test", |link, secs| shown = Some((link.to_owned(), secs))).unwrap();
        assert_eq!(shown, Some((format!("{url}/#app={}", "ab".repeat(16)), 60)));
        assert_eq!((s.cookie.as_str(), s.account.as_str(), s.login.as_str()), ("ilg_session=s3cret", "acc_me", "me"));
        drop(server);
    }

    /// The grant in what the loopback port is asked, or in an address a
    /// person pastes: only for this sign-in's ticket, and only hex.
    #[test]
    fn a_grant_is_read_only_for_its_ticket() {
        let t = "ab12";
        assert_eq!(grant_in("/arugula-signin?ticket=ab12&grant=00ff", t).as_deref(), Some("00ff"));
        assert_eq!(
            grant_in("  http://127.0.0.1:5000/arugula-signin?grant=00ff&ticket=ab12 \n", t).as_deref(),
            Some("00ff")
        );
        assert_eq!(grant_in("/arugula-signin?ticket=ab12&grant=00ff HTTP/1.1", t).as_deref(), Some("00ff"));
        assert_eq!(grant_in("/arugula-signin?ticket=ffff&grant=00ff", t), None, "another sign-in's");
        assert_eq!(grant_in("/arugula-signin?ticket=ab12&grant=<script>", t), None);
        assert_eq!(grant_in("/arugula-signin?ticket=ab12&grant=", t), None);
        assert_eq!(grant_in("no address", t), None);
    }

    /// A signed-in CLI asks control with its session's cookie and control's
    /// origin, never a signature, and says how to sign in again once the
    /// session has ended; a device signs.
    #[test]
    fn a_session_asks_with_its_cookie() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let at = url.clone();
        let mut n = 0;
        let server = control(l, move |a| {
            n += 1;
            match n {
                1 => {
                    assert_eq!(a.header("cookie"), Some("ilg_session=s"));
                    assert_eq!(a.header("origin"), Some(at.as_str()));
                    assert!(a.header("x-arugula-auth").is_none());
                    Some((200, String::new(), r#"{"daemons":[]}"#.into()))
                }
                2 => Some((401, String::new(), r#"{"error":"sign in again"}"#.into())),
                3 => {
                    assert!(a.header("cookie").is_none());
                    assert!(a.header("x-arugula-auth").is_some());
                    Some((200, String::new(), "{}".into()))
                }
                _ => None,
            }
        });
        let e = Enrolled::for_tests_session(url.clone(), "ilg_session=s");
        assert_eq!(e.api("GET", "/api/directory", None).unwrap(), json!({ "daemons": [] }));
        let err = e.api("GET", "/api/directory", None).unwrap_err();
        assert!(format!("{err:#}").contains("`arugula login --browser` again"), "{err:#}");
        Enrolled::for_tests(url).api("GET", "/api/directory", None).unwrap();
        drop(server);
    }

    /// The page's rule (`keyAfterTokens`): a device uses its key only when
    /// control gives no tokens at all (404) or the daemon is from before
    /// them; signed out, not let in or a refused token, the key wouldn't fix.
    #[test]
    fn a_device_falls_back_to_its_key_only_where_tokens_cant_work() {
        let no = |status| anyhow::Error::from(NoToken { status, said: "x".into() });
        let refused = |code: &str| anyhow::Error::from(Refused { code: code.into(), message: "x".into() });
        assert!(key_after_tokens(&no(404)));
        assert!(key_after_tokens(&refused("malformed").context("can't reach geek")));
        assert!(!key_after_tokens(&no(401)));
        assert!(!key_after_tokens(&no(429)));
        assert!(!key_after_tokens(&refused("machine-token")));
        assert!(!key_after_tokens(&anyhow::anyhow!("connection refused")));
        assert!(answered(&no(401)) && answered(&refused("machine-token")));
        assert!(!answered(&anyhow::anyhow!("timed out")));
    }

    /// Each entry's Noise key as control gives it (#994), else from the
    /// certificate beside it: a hosted machine's, or another account's.
    #[test]
    fn a_machines_noise_key_is_controls_word() {
        let key = "ab".repeat(32);
        let given = json!({ "id": "d1", "name": "geek", "noise": key });
        assert_eq!(listed(&given).unwrap().noise.as_deref(), Some(key.as_str()));
        let hosted = json!({ "id": "h1", "name": "box", "hosted": { "cert": { "noise": key } } });
        assert_eq!(listed(&hosted).unwrap().noise.as_deref(), Some(key.as_str()));
        let theirs = json!({
            "id": "d2", "name": "theirs", "account": "b",
            "chain": { "certs": [
                { "device": "other", "kind": "daemon", "noise": "00".repeat(32) },
                { "device": "d2", "kind": "browser", "noise": "11".repeat(32) },
                { "device": "d2", "kind": "daemon", "noise": key },
            ] },
        });
        assert_eq!(listed(&theirs).unwrap().noise.as_deref(), Some(key.as_str()));
        let own = json!({ "id": "d3", "name": "mine" });
        assert_eq!(listed(&own).unwrap().noise, None, "from /api/devices when it's opened");
        assert_eq!(noise_key(&key), Some([0xab; 32]));
        assert_eq!(noise_key("abc"), None);
        assert_eq!(noise_key(&"zz".repeat(32)), None);
    }
}
