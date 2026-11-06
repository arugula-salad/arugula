//! A chant→Fly deploy's approval as chant's own gate statement (#1985,
//! aru-126, INTENTIUS/chant#4646 and #4697).
//!
//! The deploy's plan runs first, on a box with no Fly token (the plan
//! box): `chant run <op>` stops at the op's gate and records its pending
//! fact, with chant's plan digest, on `chant/lifecycle`. What it recorded
//! is a [`Gate`]. The owner's passkey then signs chant's gate statement
//! for that gate (what `chant approve <op> <gate> --statement` prints), so
//! one assertion is both chant's approval and Arugula's:
//!
//! ```text
//! <op>
//! <gate>
//! <environment, or empty>
//! <chant's plan digest>
//! arugula:<the approver's account>
//! <the approval's time, ISO 8601 UTC with milliseconds>
//! relayed-by arugula
//! checkpoint <digest>                       (only when the gate resumes)
//! binding arugula deploy <dep id> op <operation hash> expires <ms>
//! ```
//!
//! The lines are LF-joined with no final newline, as chant's
//! `gateSealPayload` makes them; the WebAuthn challenge is SHA-256 of the
//! bytes, base64url without padding, as chant's `webAuthnChallenge`. The
//! binding line (chant 0.124.64 and newer, seal version 3) holds what
//! chant doesn't read and Arugula checks: the deploy, the hash of its
//! operation (`arugula deploy op v1`) and when the apply must start by. The
//! apply box hands the same signature to `chant approve --signature`, and
//! chant checks it against the passkeys `.chant/trust.json` lists for
//! `arugula:<account>` (`arugula chant trust-entry` prints one).

use serde::{Deserialize, Serialize};

/// What the approver is called in chant: `arugula:` and their account.
pub const PRINCIPAL_PREFIX: &str = "arugula:";
/// Who relays the approval to chant (`--relayed-by`).
pub const RELAY: &str = "arugula";
/// The oldest chant the driver runs: the first that signs and checks a
/// binding line (chant#4697).
pub const FLOOR: &str = "0.124.64";
/// The longest binding chant takes, in bytes.
pub const BINDING_MAX: usize = 512;

/// Whether a `driver` deploy (a roll back or not) is approved with chant's
/// statement (v3): a chant→Fly apply, which stops at a gate of chant's. A
/// roll back goes through no gate.
pub fn approved_in_chant(driver: &str, roll_back: bool) -> bool {
    driver == crate::drivers::CHANT_FLY && !roll_back
}

/// The approver's principal in chant.
pub fn principal(account: &str) -> String {
    format!("{PRINCIPAL_PREFIX}{account}")
}

/// The binding line's text: the deploy, its operation's hash and the
/// apply's last start, in ms since the epoch.
pub fn binding(deploy: &str, op: &str, expires: u64) -> String {
    format!("arugula deploy {deploy} op {op} expires {expires}")
}

/// The gate a chant→Fly deploy's plan stopped at, as its plan box read it
/// from the pending fact chant's run recorded on `chant/lifecycle`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Gate {
    /// The chant that planned it (`x.y.z`).
    pub chant: String,
    /// The op, and its gate.
    pub op: String,
    pub gate: String,
    /// The pending fact's environment: a component gate's; an op's has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// chant's plan digest (`jcs1-sha256:…`, or an older run's `sha256:…`).
    pub plan: String,
    /// The checkpoint a resuming gate carries its outputs from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
    /// The chant run that recorded the pending fact.
    pub run: String,
    /// When it did, as chant wrote it: an approval's time can't be earlier.
    pub recorded: String,
}

/// One word for a statement's line: printable ASCII, no space.
fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// A digest as chant writes one: `sha256:` (or `jcs1-sha256:`) and 64 hex.
fn digest_ok(s: &str) -> bool {
    let hex = s.strip_prefix("jcs1-sha256:").or_else(|| s.strip_prefix("sha256:"));
    hex.is_some_and(|h| h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Whether `v` is chant `x.y.z` at `floor` or newer.
pub fn at_least(v: &str, floor: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u64>> {
        let parts: Vec<u64> = s.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        (parts.len() == 3).then_some(parts)
    };
    matches!((parse(v), parse(floor)), (Some(a), Some(b)) if a >= b)
}

impl Gate {
    /// Each line one word, the plan a digest, its time chant's, and planned
    /// by a chant that checks a binding.
    pub fn well_formed(&self) -> bool {
        [&self.op, &self.gate, &self.run].into_iter().all(|f| word(f))
            && self.environment.as_ref().is_none_or(|e| word(e))
            && self.checkpoint.as_ref().is_none_or(|c| word(c))
            && digest_ok(&self.plan)
            && timestamp_ms(&self.recorded).is_some()
            && at_least(&self.chant, FLOOR)
    }

    /// chant's gate statement for `account`'s approval at `at` with
    /// `binding`: the bytes the passkey signs.
    pub fn statement(&self, account: &str, at: &str, binding: &str) -> String {
        let mut s = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\nrelayed-by {RELAY}",
            self.op,
            self.gate,
            self.environment.as_deref().unwrap_or(""),
            self.plan,
            principal(account),
            at
        );
        if let Some(c) = &self.checkpoint {
            s.push_str(&format!("\ncheckpoint {c}"));
        }
        s.push_str(&format!("\nbinding {binding}"));
        s
    }
}

/// A passkey's public key as chant's `.chant/trust.json` takes one: DER
/// SubjectPublicKeyInfo, base64url. `None` for a key it can't be.
fn spki(e: &crate::passkey::Event) -> Option<String> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let key = b64.decode(e.key.trim_end_matches('=')).ok()?;
    let der = match (e.alg, e.key_format.as_str()) {
        // id-Ed25519, then the 32-byte key.
        (crate::passkey::EDDSA, "raw") if key.len() == 32 => {
            [hex::decode("302a300506032b6570032100").ok()?, key].concat()
        }
        // id-ecPublicKey, prime256v1, then the uncompressed point.
        (crate::passkey::ES256, "sec1") if key.len() == 65 && key[0] == 4 => {
            [hex::decode("3059301306072a8648ce3d020106082a8648ce3d030107034200").ok()?, key].concat()
        }
        (crate::passkey::RS256, "spki") => key,
        _ => return None,
    };
    Some(b64.encode(der))
}

/// The `passkeys` entries of chant's `.chant/trust.json` for `log`'s
/// account (`arugula chant trust-entry`): one for each passkey live in it,
/// and each RP it was made for, as `arugula:<account>`, its key as DER
/// SubjectPublicKeyInfo, and the origins an assertion may come from (the
/// RP's own site, `control` when it's that site's origin, and `related`:
/// the related origins control lists, the platform page among them). A log
/// that doesn't hold together ([`crate::passkey::check_chain`]) gives none:
/// its refusal.
pub fn trust_entries(
    log: &crate::passkey::Log,
    control: &str,
    related: &[String],
) -> Result<Vec<serde_json::Value>, crate::passkey::Refusal> {
    let live = crate::passkey::live(log, u64::MAX)?;
    let host_of = |origin: &str| -> Option<String> {
        let rest = origin.split_once("://")?.1;
        let host = rest.split(['/', ':']).next()?;
        (!host.is_empty()).then(|| host.to_owned())
    };
    let mut out = vec![];
    for e in live {
        let Some(key) = spki(e) else { continue };
        for rp in e.rp.split(' ').filter(|r| !r.is_empty()) {
            let own = if host_of(control).as_deref() == Some(rp) {
                control.trim_end_matches('/').to_owned()
            } else {
                format!("https://{rp}")
            };
            let mut origins = vec![own];
            for o in related {
                if !origins.contains(o) {
                    origins.push(o.clone());
                }
            }
            out.push(serde_json::json!({
                "principal": principal(&log.account),
                "id": e.credential,
                "publicKey": key,
                "rpId": rp,
                "origins": origins,
            }));
        }
    }
    Ok(out)
}

/// Days from 1970-01-01 to `y-m-d` (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `at`, a time as chant writes one (`2026-11-02T10:00:00.000Z`), in ms
/// since the epoch: `None` for any other spelling.
pub fn timestamp_ms(at: &str) -> Option<u64> {
    let b = at.as_bytes();
    if b.len() != 24 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    if b[19] != b'.' || b[23] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| -> Option<i64> {
        let s = at.get(r)?;
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    let (y, mo, d, h, mi, s, ms) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?, n(20..23)?);
    if !(1..=12).contains(&mo) || d < 1 || h > 23 || mi > 59 || s > 59 {
        return None;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if d > month_days[(mo - 1) as usize] {
        return None;
    }
    let ms = (((days_from_civil(y, mo, d) * 24 + h) * 60 + mi) * 60 + s) * 1000 + ms;
    u64::try_from(ms).ok()
}

/// `ms` since the epoch as chant writes a time.
pub fn timestamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn gate() -> Gate {
        Gate {
            chant: "0.124.68".into(),
            op: "release".into(),
            gate: "ship".into(),
            environment: None,
            plan: format!("jcs1-sha256:{}", "ab".repeat(32)),
            checkpoint: None,
            run: "run-1".into(),
            recorded: "2026-11-02T10:00:00.000Z".into(),
        }
    }

    /// chant's own `gateSealPayload`, line for line (chant 0.124.68).
    #[test]
    fn the_statement_is_chants() {
        let b = binding("dep_1", &"cd".repeat(32), 1_800_000_000_000);
        assert_eq!(b, format!("arugula deploy dep_1 op {} expires 1800000000000", "cd".repeat(32)));
        assert!(b.len() <= BINDING_MAX);
        assert_eq!(
            gate().statement("acct1", "2026-11-02T10:05:00.000Z", &b),
            format!(
                "release\nship\n\njcs1-sha256:{}\narugula:acct1\n2026-11-02T10:05:00.000Z\nrelayed-by arugula\nbinding {b}",
                "ab".repeat(32)
            )
        );
        // chant 0.124.68's own `gateStatement` over these fields says this
        // challenge (its `webAuthnChallenge`).
        {
            use base64::Engine as _;
            use sha2::Digest as _;
            let text = gate().statement("acct1", "2026-11-02T10:05:00.000Z", &b);
            let ch = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(text.as_bytes()));
            assert_eq!(ch, "UCuLnQ-fpQB2r24nOSODR-7fJT3EP8XUtlvKhWiB42M");
        }
        let resumes = Gate { environment: Some("prod".into()), checkpoint: Some("sha256:00".into()), ..gate() };
        assert!(resumes.statement("a", "t", "b").ends_with("\nt\nrelayed-by arugula\ncheckpoint sha256:00\nbinding b"));
        assert!(resumes.statement("a", "t", "b").starts_with("release\nship\nprod\n"));
    }

    #[test]
    fn a_gate_is_well_formed_only_from_a_chant_that_binds() {
        assert!(gate().well_formed());
        assert!(!Gate { chant: "0.124.63".into(), ..gate() }.well_formed(), "no binding line before 0.124.64");
        assert!(Gate { chant: "0.125.0".into(), ..gate() }.well_formed());
        assert!(!Gate { plan: "sha256:ab".into(), ..gate() }.well_formed());
        assert!(Gate { plan: format!("sha256:{}", "ab".repeat(32)), ..gate() }.well_formed());
        assert!(!Gate { op: "two words".into(), ..gate() }.well_formed());
        assert!(!Gate { environment: Some(String::new()), ..gate() }.well_formed());
        assert!(!Gate { recorded: "yesterday".into(), ..gate() }.well_formed());
    }

    #[test]
    fn times_read_and_write_as_chant_writes_them() {
        for (at, ms) in [
            ("1970-01-01T00:00:00.000Z", 0),
            ("2026-11-02T10:05:00.123Z", 1_793_613_900_123),
            ("2000-02-29T23:59:59.999Z", 951_868_799_999),
        ] {
            assert_eq!(timestamp_ms(at), Some(ms), "{at}");
            assert_eq!(timestamp(ms), at);
        }
        for bad in [
            "2026-11-02T10:05:00Z",
            "2026-11-02T10:05:00.000+00:00",
            "2026-02-30T10:05:00.000Z",
            "2026-13-01T10:05:00.000Z",
            "2026-11-02 10:05:00.000Z",
            "+026-11-02T10:05:00.000Z",
        ] {
            assert_eq!(timestamp_ms(bad), None, "{bad}");
        }
    }

    /// `arugula chant trust-entry`'s entries: the live passkeys only, as
    /// chant reads them (an Ed25519 key's SPKI verifies chant's way).
    #[test]
    fn trust_entries_are_the_live_passkeys_as_chant_reads_them() {
        use crate::passkey::{Change, Draft, EDDSA, Log, NO_AAGUID};
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let key = |n: u8| ed25519_dalek::SigningKey::from_bytes(&[n; 32]).verifying_key().to_bytes().to_vec();
        let draft = |n: u8, at: u64| Draft {
            account: "acct1".into(),
            event: Change::Added,
            credential: b64.encode([n; 16]),
            rp: "control.test".into(),
            alg: EDDSA,
            key: key(n),
            aaguid: NO_AAGUID.into(),
            at,
            by: "signup".into(),
        };
        let mut log = Log { account: "acct1".into(), ..Default::default() };
        log.events.push(draft(1, 1).after(None));
        log.events.push(draft(2, 2).after(log.events.last()));
        let removed = log.events[0].removal(3, "reset").placed_after(log.events.last());
        log.events.push(removed);
        let related = vec!["https://app.test".to_owned()];
        let got = trust_entries(&log, "https://control.test", &related).unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["principal"], "arugula:acct1");
        assert_eq!(got[0]["id"], b64.encode([2u8; 16]));
        assert_eq!(got[0]["rpId"], "control.test");
        assert_eq!(got[0]["origins"], serde_json::json!(["https://control.test", "https://app.test"]));
        let der = b64.decode(got[0]["publicKey"].as_str().unwrap()).unwrap();
        assert_eq!(der.len(), 44);
        assert_eq!(&der[12..], key(2).as_slice());
        // A dev control on a port: its own origin, as the CLI knows it.
        let dev = trust_entries(&log, "http://control.test:7690/", &[]).unwrap();
        assert_eq!(dev[0]["origins"], serde_json::json!(["http://control.test:7690"]));
        // A log that doesn't hold together prints nothing: a key swapped
        // in, its hash left; or its removal dropped, the chain cut there.
        let mut swapped = log.clone();
        swapped.events[1].key = b64.encode(key(9));
        assert_eq!(trust_entries(&swapped, "https://control.test", &related), Err(crate::passkey::Refusal::Chain(1)));
        let mut cut = log.clone();
        cut.events.remove(1);
        assert!(trust_entries(&cut, "https://control.test", &related).is_err());
    }
}
