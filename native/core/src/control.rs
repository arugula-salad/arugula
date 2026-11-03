//! Control, as a native app reaches it: pairing by code from a signed-in
//! browser, and requests signed with the device's key (aru-131).

use std::fmt;

use anyhow::{Context, Result, bail, ensure};
use arugula_control_wire as wire;
use arugula_e2e::{
    Cert, Kind, Revocation, Trust,
    cert::{self, join_proof_body, request_auth},
    keys::DeviceKeys,
    now_ms,
};
use serde_json::Value;

use crate::net::Site;

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

/// A device paired to an account: signs every request with its key.
pub struct Control {
    pub url: String,
    pub(crate) site: Site,
    pub keys: DeviceKeys,
    pub cert: Cert,
    /// The account's root, pinned when it paired, which every certificate is
    /// checked back to. Control's word about who's trusted isn't enough.
    pub trust: Trust,
}

impl Control {
    pub fn new(url: &str, keys: DeviceKeys, cert: Cert, trust: Trust) -> Result<Self> {
        let url = url.trim_end_matches('/').to_owned();
        let site = Site::parse(&url)?;
        Ok(Self { url, site, keys, cert, trust })
    }

    /// A signed request: its JSON, or [`Refused`].
    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let bytes = body.map(serde_json::to_vec).transpose()?.unwrap_or_default();
        let auth = request_auth(&self.keys, method, path, &bytes);
        let mut headers = vec![("x-arugula-auth", auth.as_str()), (wire::FEATURES_HEADER, wire::APP_DEVICES)];
        if body.is_some() {
            headers.push(("Content-Type", "application/json"));
        }
        let (status, got) = self.site.request(method, path, &headers, &bytes)?;
        let v: Value = serde_json::from_slice(&got).unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let error = v["error"].as_str().map_or_else(|| String::from_utf8_lossy(&got).into_owned(), str::to_owned);
            let reason = v["reason"].as_str().map(str::to_owned);
            return Err(Refused { status, error, reason }.into());
        }
        Ok(v)
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.call("GET", path, None)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.call("POST", path, Some(body))
    }

    /// Who's trusted now, by the account's pinned root and not by control's
    /// say: what's listed that doesn't chain back to it counts for nothing.
    pub fn trusted(&self) -> Result<cert::Trusted> {
        let v = self.get("/api/devices")?;
        let certs: Vec<Cert> = serde_json::from_value(v["certs"].clone()).context("control's device list")?;
        let revs: Vec<Revocation> =
            serde_json::from_value(v["revocations"].clone()).context("control's device list")?;
        Ok(self.trust.evaluate(&certs, &revs))
    }
}

/// A device asking to join: control has its request, and a signed-in browser
/// hasn't approved it yet.
pub struct Pairing {
    url: String,
    site: Site,
    keys: DeviceKeys,
    ask: Cert,
    code: String,
    poll: String,
    secret: String,
    pub expires_in_secs: u64,
}

impl Pairing {
    /// Ask control. `keys` are new, and go in the keychain once this has
    /// answered with a [`Control`]. `name` says what it is on the device list.
    pub fn start(url: &str, keys: DeviceKeys, name: &str) -> Result<Self> {
        let url = url.trim_end_matches('/').to_owned();
        let site = Site::parse(&url)?;
        let ask = Cert::new(&keys, "", Kind::Browser, name);
        ensure!(!ask.from_app(), "\"{name}\" would read as an old desktop window (aru-131): name it something else");
        let ms = now_ms();
        let proof = wire::JoinProof { ms, sig: hex::encode(keys.signature(join_proof_body(&ask, ms).as_bytes())) };
        let body = wire::CliJoinRequest { cert: ask.clone(), proof, features: cert::JOIN_SECRET.to_owned() };
        let (status, got) =
            site.request("POST", wire::JOIN, &[("Content-Type", "application/json")], &serde_json::to_vec(&body)?)?;
        ensure!(
            status == 200,
            "control at {url} wouldn't pair this device ({status}): {}",
            String::from_utf8_lossy(&got)
        );
        let started: wire::JoinStarted = serde_json::from_slice(&got).context("control's answer to a join")?;
        ensure!(started.code == cert::join_code(&ask), "control sent a code that isn't this key's: not pairing");
        Ok(Self {
            url,
            site,
            keys,
            ask,
            code: started.code,
            poll: started.poll,
            secret: cert::join_secret(),
            expires_in_secs: started.expires_in_secs,
        })
    }

    /// What a signed-in browser opens to approve it. The secret is after the
    /// `#`, so it never reaches control.
    pub fn link(&self) -> String {
        format!("{}/#join={}", self.url, self.whole_code())
    }

    /// The whole code, for typing into a signed-in browser.
    pub fn whole_code(&self) -> String {
        format!("{}.{}", self.code, self.secret)
    }

    /// Look once: `Err(self)` while nobody has approved it. Approved, it
    /// answers only if control's answer checks out against the link's secret
    /// and the account's certificates: the device is never saved otherwise.
    pub fn poll(self) -> Result<Result<Control, Self>> {
        let path = wire::JoinPoll::path(&self.code, &self.poll);
        let (status, got) = self.site.request("GET", &path, &[], b"")?;
        if status == 404 {
            bail!("the code expired: start again for a new one");
        }
        ensure!(status == 200, "control's answer about the approval ({status})");
        let v: wire::JoinPoll = serde_json::from_slice(&got).context("control's answer about the approval")?;
        if let Some(on) = &v.rejected {
            bail!("turned down on {on}");
        }
        if !v.approved {
            return Ok(Err(self));
        }
        let cert = v.cert.context("control approved it but sent no certificate")?;
        let trust = v.trust.context("control sent no account root")?;
        ensure!(cert.same_request(&self.ask), "control sent back a certificate for a different key: not pairing");
        if let Some(r) = trust.refusal(&v.certs, &v.revocations, &cert) {
            bail!("the approval doesn't check out against the account's devices: {}", r.check());
        }
        let mac = v.mac.context("the approval came without the answer to this link's secret: not pairing")?;
        ensure!(
            cert::join_mac_ok(&self.secret, &trust, &cert, &mac),
            "the approval doesn't match the secret in this device's link: not pairing"
        );
        Control::new(&self.url, self.keys, cert, trust).map(Ok)
    }
}
