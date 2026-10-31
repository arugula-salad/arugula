//! `passkey`: an account's passkey log (DK3, aru-126): each event's text and
//! hash, and the one check of a passkey's assertion against a log, every
//! refusal included. The page's copy (`web/src/shell/e2e/passkey.ts`) runs
//! the same files (`web/scripts/passkey.test.mjs`).

use arugula_e2e::passkey::{
    Assertion, Change, Draft, EDDSA, ES256, Event, Log, NO_AAGUID, Origins, RS256, UP, UV, check_chain, verify,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::harness::{Ctx, Example, Spec, input};

pub const FILES: &[Spec] = &[
    Spec { module: "passkey", name: "bodies", run: run_bodies, make: bodies },
    Spec { module: "passkey", name: "verify", run: run_verify, make: verifies },
];

const ACCOUNT: &str = "acct_owner";
const RP: &str = "control.example";
const CONTROL: &str = "https://control.example";
const APP: &str = "https://app.example";
/// iCloud Keychain's AAGUID, for a passkey whose authenticator said.
const ICLOUD: &str = "fbfc3007154e4ecc8c0b6e020557d7bd";

/// An RSA passkey's public key (SPKI) and one assertion it made, over
/// [`approval`] from `https://app.example` for RP `control.example` with UP
/// and UV. Made once with a key that was thrown away: RSA keys can't come
/// from a seed here, and the assertion doesn't depend on the log, so these
/// stay valid whatever the log's format.
const RSA_SPKI: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA0_SvFUJnVhDsI4X8PXKX-O5lnZpoRtsqWUH7vBPsTt3rFWcMiqtMCekZI8lGrXGGq9m_LTb1FbRPFtYmxBqQsdelHl6er0Sg8a1o28PT-5iIfnUjH8bleFJk6K1iRlyo579hU5rXAs0pzFdU0WiVpWEDimacTfpHZ7F53w0sg5zvq6ed8i_5P3B9z5rBh4os82tOArdmAIJ_aZMTJLeCOX8sMSlWDpZeq69WKGKRudrQUCg-YWMiFApyGQO4PSAVwQdTLBs0NqPXsmPOuPohvstHshr91ypPg0O9DC9QT7e7912WEYTj7zjhVooYlNYK-xyp0HNVAq38AVHRJ5NewQIDAQAB";
const RSA_CLIENT_DATA: &str = "eyJ0eXBlIjoid2ViYXV0aG4uZ2V0IiwiY2hhbGxlbmdlIjoiOXVoM3dIOVgwWDhaRHdUNUFqV3htUU04eXRXT3Fqc0UxQmpuVUQ2TEpPQSIsIm9yaWdpbiI6Imh0dHBzOi8vYXBwLmV4YW1wbGUiLCJjcm9zc09yaWdpbiI6ZmFsc2V9";
const RSA_AUTH_DATA: &str = "a0VSR89O6IQunw3j9h2nuoCHSI-KE3luCkikpADknaEFAAAAAA";
const RSA_SIG: &str = "qnhbZnk1hvKxwDZG3qgE_XQhhR_d6h3Z_ZwgyVnzbuBm8iiSF6b6FkXsOew68tKSIyoiPIDfD_1dp-SPE_M_WYb72Fts8ItAIeXT7HTq_dRj-PcBb0pN-yFiGe6CtI9N7ID4EQIueWr3LjiZ6LcFfR7xtwdA6WHVpJppaGirw6R7LW0KzmYJhAU9SvYpO_RrN19sUcphwxC5Gn_ulHxQy7U0ITc5cEX7wEE2iVeduE7K9Go-JmENghj_qLgoEf5BIgUY0JeU4T8QdJbWfZ6kk3GnX1g8rhg2fkFmNvfzsGBIVvGE94me7UvinTxFbAaiRqmAMMLqGeKpYACjZEOkRw";

/// The challenge every approval case signs: SHA-256 of an approval's bytes.
fn approval() -> String {
    B64.encode(Sha256::digest(b"an approval's bytes"))
}

/// A software passkey: Ed25519 from seed byte `n`, or P-256 from scalar
/// bytes `n`, its id `n` sixteen times. Signs deterministically.
enum Signer {
    Ed(ed25519_dalek::SigningKey),
    Es(p256::ecdsa::SigningKey),
}

struct Pk {
    id: String,
    alg: i64,
    public: Vec<u8>,
    signer: Option<Signer>,
}

impl Pk {
    fn ed(n: u8) -> Self {
        let k = ed25519_dalek::SigningKey::from_bytes(&[n; 32]);
        Pk {
            id: B64.encode([n; 16]),
            alg: EDDSA,
            public: k.verifying_key().to_bytes().to_vec(),
            signer: Some(Signer::Ed(k)),
        }
    }

    fn es(n: u8) -> Self {
        let k = p256::ecdsa::SigningKey::from_slice(&[n; 32]).unwrap();
        let public = k.verifying_key().to_sec1_point(false).as_bytes().to_vec();
        Pk { id: B64.encode([n; 16]), alg: ES256, public, signer: Some(Signer::Es(k)) }
    }

    fn rsa() -> Self {
        Pk { id: B64.encode([0x52; 16]), alg: RS256, public: B64.decode(RSA_SPKI).unwrap(), signer: None }
    }

    fn added(&self, account: &str, rp: &str, aaguid: &str, at: u64, by: &str) -> Draft {
        Draft {
            account: account.into(),
            event: Change::Added,
            credential: self.id.clone(),
            rp: rp.into(),
            alg: self.alg,
            key: self.public.clone(),
            aaguid: aaguid.into(),
            at,
            by: by.into(),
        }
    }

    /// An assertion over `challenge`, its client data `cd` (JSON) and
    /// authenticator data for `rp` with `flags`.
    fn sign_raw(&self, cd: &[u8], rp: &str, flags: u8) -> Assertion {
        let mut data = Sha256::digest(rp.as_bytes()).to_vec();
        data.push(flags);
        data.extend_from_slice(&0u32.to_be_bytes());
        let mut msg = data.clone();
        msg.extend_from_slice(&Sha256::digest(cd));
        let sig = match self.signer.as_ref().expect("a key that signs") {
            Signer::Ed(k) => {
                use ed25519_dalek::Signer as _;
                k.sign(&msg).to_bytes().to_vec()
            }
            Signer::Es(k) => {
                use p256::ecdsa::signature::Signer as _;
                let s: p256::ecdsa::Signature = k.sign(&msg);
                s.to_der().as_bytes().to_vec()
            }
        };
        Assertion {
            id: self.id.clone(),
            client_data_json: B64.encode(cd),
            authenticator_data: B64.encode(&data),
            signature: B64.encode(sig),
        }
    }

    fn sign(&self, challenge: &str, origin: &str) -> Assertion {
        self.sign_with(challenge, origin, json!({}), RP, UP | UV)
    }

    fn sign_with(&self, challenge: &str, origin: &str, extra: Value, rp: &str, flags: u8) -> Assertion {
        let mut cd = json!({ "type": "webauthn.get", "challenge": challenge, "origin": origin, "crossOrigin": false });
        cd.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        self.sign_raw(&serde_json::to_vec(&cd).unwrap(), rp, flags)
    }

    fn rsa_assertion(&self) -> Assertion {
        Assertion {
            id: self.id.clone(),
            client_data_json: RSA_CLIENT_DATA.into(),
            authenticator_data: RSA_AUTH_DATA.into(),
            signature: RSA_SIG.into(),
        }
    }
}

/// `e` placed at the end of `log`.
fn push(log: &mut Log, e: Event) {
    let e = e.placed_after(log.events.last());
    log.events.push(e);
}

/// `d` at the end of `log`, signed by `signer` (`by passkey`).
fn push_signed(log: &mut Log, e: Event, signer: &Pk) {
    let e = e.placed_after(log.events.last());
    let proof = signer.sign(&e.challenge(), CONTROL);
    log.events.push(e.with_proof(proof));
}

struct Keys {
    es: Pk,
    rsa: Pk,
    ed: Pk,
    ed3: Pk,
    late: Pk,
}

fn keys() -> Keys {
    Keys { es: Pk::es(0x11), rsa: Pk::rsa(), ed: Pk::ed(2), ed3: Pk::ed(3), late: Pk::ed(4) }
}

/// The owner's log:
/// 0. ES256 added at 1000, signing up (iCloud Keychain's AAGUID);
/// 1. RS256 added at 1500 after a fresh GitHub sign-in;
/// 2. Ed25519 added at 2000, the ES256 one signing for it;
/// 3. the ES256 one removed at 3000, the Ed25519 one signing;
/// 4. another Ed25519 added at 4000, a browser of the account signing;
/// 5. the RS256 one removed at 6000 by a signed-in page.
fn owner_log(k: &Keys) -> Log {
    let mut log = Log { account: ACCOUNT.into(), ..Default::default() };
    push(&mut log, k.es.added(ACCOUNT, RP, ICLOUD, 1000, "signup").after(None));
    push(&mut log, k.rsa.added(ACCOUNT, RP, NO_AAGUID, 1500, "github").after(None));
    push_signed(&mut log, k.ed.added(ACCOUNT, RP, NO_AAGUID, 2000, "passkey").after(None), &k.es);
    let es_added = log.events[0].clone();
    push_signed(&mut log, es_added.removal(3000, "passkey"), &k.ed);
    push(&mut log, k.ed3.added(ACCOUNT, RP, NO_AAGUID, 4000, "device dev_laptop").after(None));
    let rsa_added = log.events[1].clone();
    push(&mut log, rsa_added.removal(6000, "session"));
    log
}

/// An account from before the log: one Ed25519 passkey, its RP either of
/// control's two sites, written by the backfill at 9000.
fn backfilled_log(k: &Keys) -> Log {
    let mut log = Log { account: "acct_old".into(), began: Some(9000), ..Default::default() };
    push(&mut log, k.ed.added("acct_old", "control.example old.example", NO_AAGUID, 500, "backfill 9000").after(None));
    log
}

fn origins() -> Origins {
    Origins::from([(RP.to_owned(), vec![CONTROL.to_owned(), APP.to_owned()])])
}

fn both_sites() -> Origins {
    Origins::from([
        (RP.to_owned(), vec![CONTROL.to_owned()]),
        ("old.example".to_owned(), vec!["https://old.example".to_owned()]),
    ])
}

// ---- bodies ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyIn {
    event: String,
}

fn run_bodies(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: BodyIn = input(v)?;
    let e: Event = ctx.get(&i.event)?;
    Ok(json!({ "body": e.body(), "proof_line": e.proof_line(), "hash": e.compute_hash(), "challenge": e.challenge() }))
}

fn bodies() -> Example {
    let mut ex = Example::new(
        "passkey",
        "bodies",
        run_bodies,
        "An event of an account's passkey log, as text: `arugula passkey log v1`, then account, seq, prev (the \
         event before's hash, or 64 zeros), event (added or removed), credential, rp, alg, key (its format and \
         base64url), aaguid, at and by, one a line, each ending in a newline. A `by passkey` event has a proof \
         line after it, `proof <credential> <clientDataJSON> <authenticatorData> <signature>`, the signer's \
         assertion. Its hash is SHA-256 of the body and the proof line, hex; what the proof signs (its \
         challenge) is SHA-256 of the body alone, base64url.",
        &[
            "arugula_e2e::passkey::Event::body",
            "arugula_e2e::passkey::Event::proof_line",
            "arugula_e2e::passkey::Event::compute_hash",
            "arugula_e2e::passkey::Event::challenge",
        ],
    )
    .shapes(
        "event: an event's label",
        "body, proof_line (empty but on a `by passkey` event), hash (hex), challenge (base64url)",
    );
    let k = keys();
    let log = owner_log(&k);
    let names = [
        "es256 signing up",
        "rs256 after github",
        "ed25519 signed for",
        "es256 removed",
        "by a device",
        "rs256 removed",
    ];
    for (i, (e, name)) in log.events.iter().zip(names).enumerate() {
        let label = ex.object(&format!("owner_{i}"), e);
        let got = ex.computed(&format!("the owner's event {i}: {name}"), json!({ "event": label }));
        assert_eq!(got["hash"], e.hash, "an event's hash is its body's and proof's");
    }
    let old = backfilled_log(&k);
    let label = ex.object("backfilled_0", &old.events[0]);
    ex.computed("a backfilled event, its RP either site", json!({ "event": label }));
    ex
}

// ---- verify ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyIn {
    log: String,
    account: String,
    assertion: String,
    challenge: String,
    at: u64,
    origins: String,
}

fn run_verify(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: VerifyIn = input(v)?;
    let log: Log = ctx.get(&i.log)?;
    let a: Assertion = ctx.get(&i.assertion)?;
    let origins: Origins = ctx.get(&i.origins)?;
    // The chain alone says the same of a broken log.
    let chain = check_chain(&log);
    Ok(match verify(&log, &i.account, &a, &i.challenge, i.at, &origins) {
        Ok(v) => {
            anyhow::ensure!(chain.is_ok(), "verified against a log whose chain fails");
            json!({ "ok": true, "added": v.key.seq, "rp": v.rp })
        }
        Err(r) => {
            if let arugula_e2e::passkey::Refusal::Chain(n) = r {
                anyhow::ensure!(chain == Err(r), "the chain check disagrees");
                json!({ "ok": false, "refused": r.code(), "event": n })
            } else {
                json!({ "ok": false, "refused": r.code() })
            }
        }
    })
}

fn ok(added: u64, rp: &str) -> Value {
    json!({ "ok": true, "added": added, "rp": rp })
}

fn refused(code: &str) -> Value {
    json!({ "ok": false, "refused": code })
}

fn chain(n: u64) -> Value {
    json!({ "ok": false, "refused": "chain", "event": n })
}

fn verifies() -> Example {
    let mut ex = Example::new(
        "passkey",
        "verify",
        run_verify,
        "A passkey's assertion checked against its account's log, trusting nothing else (DK3, aru-126). The \
         log must be the account's asked about (`account`). Then the whole log, in order: each event is the log's account's, its seq is its place (from 0), its prev \
         the hash of the one before (64 zeros first), its hash its own (SHA-256 of body and proof line), its \
         time not before the one before's, its fields fit their lines (an RP may list several sites, space \
         separated, only on a `by backfill <ms>` event), its key's format its alg's (sec1 for -7, raw for -8, \
         spki for -257), its `by` one of passkey, signup, github, device <id>, session, reset, deleted or \
         backfill <ms>, with a proof exactly when it's passkey; a passkey is added only when it isn't live and \
         removed only when it is, with its key. A proof is checked as an assertion below, by a passkey live \
         just before the event, over SHA-256 of the event's body (base64url), except that its origin must be \
         its RP's own (http or https, the RP as host, any port), not one the verifier pins: whether a log holds \
         together never depends on a verifier's configuration. The first event that fails is \
         `chain`, with its index. Then the assertion's passkey must be live at `at`: added by an event at or \
         before `at`, and not removed by one at or before it (`not_live`). Then, in order: clientDataJSON \
         base64url JSON with type, challenge and origin (`client_data`); type webauthn.get (`ceremony`); the \
         challenge exactly (`challenge`); not crossOrigin true, nor a topOrigin other than origin (`framed`); \
         authenticator data base64url of 37 bytes or more (`auth_data`); its first 32 bytes SHA-256 of the \
         passkey's RP, or of one of them (`rp`); the origin one the verifier allows for that RP (`origin`); \
         flags with UP (0x01) and UV (0x04) (`user`); a key that reads (`key`); the signature, over \
         authenticator data then SHA-256 of clientDataJSON: ES256 DER over P-256, Ed25519 strict, RS256 PKCS#1 \
         v1.5 with SHA-256 (`signature`). Base64url may come padded, but its trailing bits must be zero. The \
         client data is read strictly: a field it reads (type, challenge, origin, crossOrigin, topOrigin) twice \
         is `client_data`; other fields are ignored, twice or not. An ES256 signature's DER integers must be \
         minimal and positive. No provider policy: any AAGUID counts. Ed25519 is checked strictly (Rust's \
         verify_strict); WebCrypto may take small-order keys and non-canonical signatures it would refuse, and \
         no case here has one.",
        &["arugula_e2e::passkey::verify", "arugula_e2e::passkey::check_chain"],
    )
    .shapes(
        "log, assertion and origins: labels; account: whose log it should be (origins: { rp: [origins] }, the verifier's pinned map); challenge: \
         base64url; at: ms, the verifier's own record of when it arrived",
        "{ ok: true, added: the seq of the passkey's added event, rp: the RP it was for }, or { ok: false, \
         refused: a code, event: the index, for chain }",
    );
    let k = keys();
    let ch = approval();
    let log = owner_log(&k);
    ex.object("owner", &log);
    ex.object("control_and_app", &origins());
    ex.object("control_only", &Origins::from([(RP.to_owned(), vec![CONTROL.to_owned()])]));
    let case = |ex: &mut Example, about: &str, log: &str, a: &str, a_obj: &Assertion, at: u64, o: &str, want: Value| {
        ex.object(a, a_obj);
        let account = if log == "backfilled" { "acct_old" } else { ACCOUNT };
        ex.case(
            about,
            json!({ "log": log, "account": account, "assertion": a, "challenge": ch, "at": at, "origins": o }),
            want,
        );
    };

    let es = k.es.sign(&ch, APP);
    case(
        &mut ex,
        "ES256 from the platform's page, before its removal",
        "owner",
        "es_app",
        &es,
        2999,
        "control_and_app",
        ok(0, RP),
    );
    case(
        &mut ex,
        "ES256 at its removal's time: removed before at",
        "owner",
        "es_app",
        &es,
        3000,
        "control_and_app",
        refused("not_live"),
    );
    case(&mut ex, "ES256 before it was added", "owner", "es_app", &es, 999, "control_and_app", refused("not_live"));
    let ed = k.ed.sign(&ch, APP);
    case(
        &mut ex,
        "Ed25519, added by the ES256 one's proof",
        "owner",
        "ed_app",
        &ed,
        5000,
        "control_and_app",
        ok(2, RP),
    );
    let ed_control = k.ed.sign(&ch, CONTROL);
    case(
        &mut ex,
        "Ed25519 from control's own page",
        "owner",
        "ed_control",
        &ed_control,
        5000,
        "control_and_app",
        ok(2, RP),
    );
    case(
        &mut ex,
        "from the platform's page where the verifier allows only control's",
        "owner",
        "ed_app",
        &ed,
        5000,
        "control_only",
        refused("origin"),
    );
    let ed3 = k.ed3.sign(&ch, APP);
    case(
        &mut ex,
        "a passkey a device signed for, just before it was added",
        "owner",
        "ed3_app",
        &ed3,
        3999,
        "control_and_app",
        refused("not_live"),
    );
    case(
        &mut ex,
        "a passkey a device signed for, as it was added",
        "owner",
        "ed3_app",
        &ed3,
        4000,
        "control_and_app",
        ok(4, RP),
    );
    let rsa = k.rsa.rsa_assertion();
    case(&mut ex, "RS256 before a page removed it", "owner", "rsa_app", &rsa, 5999, "control_and_app", ok(1, RP));
    case(
        &mut ex,
        "RS256 after a page removed it",
        "owner",
        "rsa_app",
        &rsa,
        6000,
        "control_and_app",
        refused("not_live"),
    );
    let stranger = k.late.sign(&ch, APP);
    case(
        &mut ex,
        "a passkey that was never the account's",
        "owner",
        "stranger",
        &stranger,
        5000,
        "control_and_app",
        refused("not_live"),
    );

    let bad = |extra: Value, rp: &str, flags: u8| k.ed.sign_with(&ch, APP, extra, rp, flags);
    case(
        &mut ex,
        "from another site",
        "owner",
        "evil_origin",
        &bad(json!({ "origin": "https://evil.example" }), RP, UP | UV),
        5000,
        "control_and_app",
        refused("origin"),
    );
    case(&mut ex, "UV not set", "owner", "no_uv", &bad(json!({}), RP, UP), 5000, "control_and_app", refused("user"));
    case(&mut ex, "UP not set", "owner", "no_up", &bad(json!({}), RP, UV), 5000, "control_and_app", refused("user"));
    case(
        &mut ex,
        "for another RP",
        "owner",
        "other_rp",
        &bad(json!({}), "example", UP | UV),
        5000,
        "control_and_app",
        refused("rp"),
    );
    case(
        &mut ex,
        "inside another site's frame",
        "owner",
        "cross_origin",
        &bad(json!({ "crossOrigin": true }), RP, UP | UV),
        5000,
        "control_and_app",
        refused("framed"),
    );
    case(
        &mut ex,
        "under another site's page",
        "owner",
        "top_origin",
        &bad(json!({ "topOrigin": "https://evil.example" }), RP, UP | UV),
        5000,
        "control_and_app",
        refused("framed"),
    );
    case(
        &mut ex,
        "its own page as topOrigin",
        "owner",
        "top_self",
        &bad(json!({ "topOrigin": APP }), RP, UP | UV),
        5000,
        "control_and_app",
        ok(2, RP),
    );
    case(
        &mut ex,
        "made, not asserted",
        "owner",
        "create",
        &bad(json!({ "type": "webauthn.create" }), RP, UP | UV),
        5000,
        "control_and_app",
        refused("ceremony"),
    );
    let other = k.ed.sign(&B64.encode(Sha256::digest(b"other bytes")), APP);
    case(&mut ex, "over other bytes", "owner", "other_bytes", &other, 5000, "control_and_app", refused("challenge"));
    let mut forged = k.late.sign(&ch, APP);
    forged.id = k.ed.id.clone();
    case(&mut ex, "signed by another key", "owner", "forged", &forged, 5000, "control_and_app", refused("signature"));
    let mut short = ed.clone();
    short.authenticator_data = B64.encode([0u8; 36]);
    case(&mut ex, "short authenticator data", "owner", "short", &short, 5000, "control_and_app", refused("auth_data"));
    let mut not_json = ed.clone();
    not_json.client_data_json = B64.encode(b"not json");
    case(
        &mut ex,
        "client data that isn't JSON",
        "owner",
        "not_json",
        &not_json,
        5000,
        "control_and_app",
        refused("client_data"),
    );
    let mut padded = ed.clone();
    padded.client_data_json.push_str(&"=".repeat((4 - padded.client_data_json.len() % 4) % 4));
    padded.authenticator_data.push_str(&"=".repeat((4 - padded.authenticator_data.len() % 4) % 4));
    case(&mut ex, "padded base64url", "owner", "padded", &padded, 5000, "control_and_app", ok(2, RP));

    // Backfilled: either site's hash is the passkey's.
    ex.object("backfilled", &backfilled_log(&k));
    ex.object("both_sites", &both_sites());
    let old = k.ed.sign_with(&ch, "https://old.example", json!({}), "old.example", UP | UV);
    case(
        &mut ex,
        "backfilled, made at the old site",
        "backfilled",
        "old_site",
        &old,
        9500,
        "both_sites",
        ok(0, "old.example"),
    );
    let new = k.ed.sign(&ch, CONTROL);
    case(&mut ex, "backfilled, made at the new site", "backfilled", "new_site", &new, 9500, "both_sites", ok(0, RP));
    let third = k.ed.sign_with(&ch, "https://third.example", json!({}), "third.example", UP | UV);
    case(
        &mut ex,
        "backfilled, a site it lists neither",
        "backfilled",
        "third_site",
        &third,
        9500,
        "both_sites",
        refused("rp"),
    );
    let old_wrong = k.ed.sign_with(&ch, CONTROL, json!({}), "old.example", UP | UV);
    case(
        &mut ex,
        "backfilled, the old site's RP from the new site's page",
        "backfilled",
        "old_rp_new_origin",
        &old_wrong,
        9500,
        "both_sites",
        refused("origin"),
    );

    // Logs that don't hold together, each refused at the event that fails.
    let tamper = |ex: &mut Example, about: &str, label: &str, f: &dyn Fn(&mut Log), want: Value| {
        let mut l = log.clone();
        f(&mut l);
        ex.object(label, &l);
        ex.case(
            about,
            json!({ "log": label, "account": ACCOUNT, "assertion": "ed_app", "challenge": ch, "at": 5000,
                    "origins": "control_and_app" }),
            want,
        );
    };
    tamper(&mut ex, "a key swapped in event 0", "swapped", &|l| l.events[0].key = l.events[2].key.clone(), chain(0));
    tamper(
        &mut ex,
        "a key swapped in event 0 and rehashed: event 1's prev breaks",
        "rehashed",
        &|l| {
            l.events[0].key = B64.encode(Pk::es(0x12).public);
            l.events[0].hash = l.events[0].compute_hash();
        },
        chain(1),
    );
    tamper(
        &mut ex,
        "an event cut out",
        "cut",
        &|l| {
            l.events.remove(1);
        },
        chain(1),
    );
    tamper(
        &mut ex,
        "another account's event",
        "other_account",
        &|l| l.events[4].account = "acct_other".into(),
        chain(4),
    );
    tamper(
        &mut ex,
        "a passkey event without its proof",
        "unproven",
        &|l| {
            l.events[2].proof = None;
            l.events[2].hash = l.events[2].compute_hash();
        },
        chain(2),
    );
    let k_es_for_late = Pk::es(0x11);
    tamper(
        &mut ex,
        "an add signed by a passkey already removed",
        "late_signer",
        &|l| {
            let e = k.late.added(ACCOUNT, RP, NO_AAGUID, 7000, "passkey").after(None).placed_after(l.events.last());
            let proof = k_es_for_late.sign(&e.challenge(), CONTROL);
            l.events.push(e.with_proof(proof));
        },
        chain(6),
    );
    tamper(
        &mut ex,
        "an event older than the one before",
        "backwards",
        &|l| push(l, k.late.added(ACCOUNT, RP, NO_AAGUID, 5000, "signup").after(None)),
        chain(6),
    );
    tamper(
        &mut ex,
        "a passkey added twice",
        "twice",
        &|l| push(l, k.ed.added(ACCOUNT, RP, NO_AAGUID, 7000, "github").after(None)),
        chain(6),
    );
    tamper(
        &mut ex,
        "a passkey removed that wasn't there",
        "removed_unknown",
        &|l| {
            let e = k.late.added(ACCOUNT, RP, NO_AAGUID, 7000, "session").after(None).removal(7000, "session");
            push(l, e);
        },
        chain(6),
    );
    tamper(
        &mut ex,
        "a line smuggled into by",
        "smuggled",
        &|l| push(l, k.late.added(ACCOUNT, RP, NO_AAGUID, 7000, "github\nby signup").after(None)),
        chain(6),
    );
    tamper(
        &mut ex,
        "a proof on an event the hub vouches for",
        "proof_on_vouched",
        &|l| {
            let e = k.late.added(ACCOUNT, RP, NO_AAGUID, 7000, "github").after(None).placed_after(l.events.last());
            let proof = k.ed.sign(&e.challenge(), CONTROL);
            l.events.push(e.with_proof(proof));
        },
        chain(6),
    );
    tamper(
        &mut ex,
        "an open RP on an event that isn't backfilled",
        "open_rp",
        &|l| push(l, k.late.added(ACCOUNT, "control.example old.example", NO_AAGUID, 7000, "github").after(None)),
        chain(6),
    );
    tamper(
        &mut ex,
        "a key whose format isn't its alg's",
        "format",
        &|l| {
            let mut e = k.late.added(ACCOUNT, RP, NO_AAGUID, 7000, "github").after(None);
            e.key_format = "sec1".into();
            push(l, e);
        },
        chain(6),
    );
    tamper(
        &mut ex,
        "an add whose proof came from the platform's page, not its RP's own",
        "proof_elsewhere",
        &|l| {
            let e = k.late.added(ACCOUNT, RP, NO_AAGUID, 7000, "passkey").after(None).placed_after(l.events.last());
            let proof = k.ed.sign_with(&e.challenge(), APP, json!({}), RP, UP | UV);
            l.events.push(e.with_proof(proof));
        },
        chain(6),
    );
    // Event 6 added later than `at` still counts toward the chain: a log
    // broken after `at` fails too.

    // The log's proofs came from control's page; a verifier that allows
    // only the platform's still takes the log.
    ex.object("app_only", &Origins::from([(RP.to_owned(), vec![APP.to_owned()])]));
    case(
        &mut ex,
        "a verifier that doesn't list the origin the log's proofs came from",
        "owner",
        "ed_app",
        &ed,
        5000,
        "app_only",
        ok(2, RP),
    );
    ex.case(
        "another account's log",
        json!({ "log": "owner", "account": "acct_other", "assertion": "ed_app", "challenge": ch, "at": 5000,
                "origins": "control_and_app" }),
        refused("account"),
    );

    // Read strictly, as Rust does.
    let mut loose = ed.clone();
    let last = loose.authenticator_data.pop().unwrap();
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let i = alphabet.find(last).unwrap();
    assert_eq!(loose.authenticator_data.len() % 4, 1, "50 characters: the last has 4 spare bits");
    loose.authenticator_data.push(alphabet.as_bytes()[i | 1] as char);
    case(
        &mut ex,
        "base64url with trailing bits set",
        "owner",
        "trailing_bits",
        &loose,
        5000,
        "control_and_app",
        refused("auth_data"),
    );
    let dup = format!(
        r#"{{"type":"webauthn.get","challenge":"b3RoZXI","challenge":"{ch}","origin":"{APP}","crossOrigin":false}}"#
    );
    let dup_a = k.ed.sign_raw(dup.as_bytes(), RP, UP | UV);
    case(
        &mut ex,
        "a field the check reads, twice",
        "owner",
        "dup_challenge",
        &dup_a,
        5000,
        "control_and_app",
        refused("client_data"),
    );
    let extra = format!(r#"{{"type":"webauthn.get","challenge":"{ch}","origin":"{APP}","x":1,"x":2}}"#);
    let extra_a = k.ed.sign_raw(extra.as_bytes(), RP, UP | UV);
    case(&mut ex, "a field it ignores, twice", "owner", "dup_other", &extra_a, 5000, "control_and_app", ok(2, RP));
    // ES256 DER: an integer with a needless leading zero, and one read as
    // negative (its sign byte dropped). k3 (ES256, seed 0x11) was removed
    // at 3000, so these are checked at 2999.
    let es_sig = |a: &Assertion| B64.decode(&a.signature).unwrap();
    let mut long = es.clone();
    let (r, sv) = der_ints(&es_sig(&es));
    let padded_s = [&[0u8][..], &sv].concat();
    long.signature = B64.encode(der(&r, &padded_s));
    case(
        &mut ex,
        "an ES256 signature with a needless leading zero",
        "owner",
        "der_long",
        &long,
        2999,
        "control_and_app",
        refused("signature"),
    );
    let padded = (0..64)
        .map(|n| k.es.sign_with(&ch, APP, json!({ "n": n }), RP, UP | UV))
        .find(|a| der_ints(&es_sig(a)).0[0] == 0)
        .expect("an r with a sign byte");
    case(
        &mut ex,
        "ES256, its r needing a sign byte",
        "owner",
        "der_signed",
        &padded,
        2999,
        "control_and_app",
        ok(0, RP),
    );
    let mut negative = padded.clone();
    let (r, sv) = der_ints(&es_sig(&padded));
    negative.signature = B64.encode(der(&r[1..], &sv));
    case(
        &mut ex,
        "ES256, its r's sign byte dropped (negative)",
        "owner",
        "der_negative",
        &negative,
        2999,
        "control_and_app",
        refused("signature"),
    );
    ex
}

/// A DER ECDSA signature's two integers, as written.
fn der_ints(sig: &[u8]) -> (Vec<u8>, Vec<u8>) {
    assert_eq!(sig[0], 0x30);
    let r_len = sig[3] as usize;
    let r = sig[4..4 + r_len].to_vec();
    let s_len = sig[5 + r_len] as usize;
    (r, sig[6 + r_len..6 + r_len + s_len].to_vec())
}

/// Two integers as a DER ECDSA signature, as given.
fn der(r: &[u8], s: &[u8]) -> Vec<u8> {
    let mut out = vec![0x30, (4 + r.len() + s.len()) as u8, 2, r.len() as u8];
    out.extend_from_slice(r);
    out.extend_from_slice(&[2, s.len() as u8]);
    out.extend_from_slice(s);
    out
}
