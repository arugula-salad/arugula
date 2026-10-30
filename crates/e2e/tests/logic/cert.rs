//! `cert`: Ed25519 checks (`verify_hex`), devices' kinds, certificates'
//! and revocations' bytes, and which devices an account trusts
//! (`Trust::evaluate`, `refusal`, `browser_chained`), bad chains included.

use arugula_e2e::{
    Cert, Kind, Revocation, Trust,
    cert::{Trusted, join_proof_body, verify_hex},
};
use ed25519_dalek::{Verifier as _, VerifyingKey};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::harness::{Ctx, Example, Spec, cert, input, keys, labels, revoke};

pub const FILES: &[Spec] = &[
    Spec { module: "cert", name: "verify_hex", run: run_verify, make: verify },
    Spec { module: "cert", name: "kinds", run: run_kinds, make: kinds },
    Spec { module: "cert", name: "app_names", run: run_app, make: app_names },
    Spec { module: "cert", name: "bodies", run: run_bodies, make: bodies },
    Spec { module: "cert", name: "check_form", run: run_form, make: check_form },
    Spec { module: "cert", name: "evaluate", run: run_evaluate, make: evaluate },
    Spec { module: "cert", name: "refusal", run: run_refusal, make: refusal },
    Spec { module: "cert", name: "browser_chained", run: run_chained, make: browser_chained },
];

const ACCT: &str = "acct";

/// A certificate in `ACCT`.
fn ac(k: &arugula_e2e::DeviceKeys, kind: Kind, name: &str, at: u64, by: &arugula_e2e::DeviceKeys) -> Cert {
    cert(k, ACCT, kind, name, at, by)
}

fn kind_str(k: Kind) -> &'static str {
    k.as_str()
}

fn parse_kind(s: &str) -> anyhow::Result<Kind> {
    Ok(serde_json::from_value(json!(s))?)
}

const KINDS: [Kind; 5] = [Kind::Browser, Kind::Cli, Kind::Daemon, Kind::Recovery, Kind::App];

// ---- verify_hex ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyIn {
    sign: String,
    message: String,
    sig: String,
}

fn run_verify(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: VerifyIn = input(v)?;
    Ok(json!({ "ok": verify_hex(&i.sign, i.message.as_bytes(), &i.sig) }))
}

fn verify() -> Example {
    let mut ex = Example::new(
        "cert",
        "verify_hex",
        run_verify,
        "Whether a signature is an Ed25519 public key's over a message: what every signed object here (a \
         certificate, a revocation, a roster, an approval, a run's event) is checked with. Strict Ed25519 \
         (RFC 8032 with ed25519-dalek's verify_strict): a non-canonical s, and a small-order (weak) public key, \
         are refused, even where a lax check would pass. Hex is read in either case; anything that isn't 32 \
         bytes of key and 64 of signature is refused, never an error.",
        &["arugula_e2e::cert::verify_hex"],
    )
    .shapes(
        "sign: the Ed25519 public key, hex; message: the signed bytes, as UTF-8 text; sig: the signature, hex",
        "ok: whether it verifies",
    );
    let k = ex.key("signer", 1);
    let other = ex.key("someone-else", 2);
    let msg = "arugula example message v1\nline two\n";
    let pk = hex::encode(k.sign_public());
    let sig = hex::encode(k.signature(msg.as_bytes()));
    let case = |ex: &mut Example, about: &str, sign: &str, message: &str, s: &str, ok: bool| {
        ex.case(about, json!({ "sign": sign, "message": message, "sig": s }), json!({ "ok": ok }));
    };
    case(&mut ex, "the signer's signature over the message", &pk, msg, &sig, true);
    case(
        &mut ex,
        "the same signature in upper-case hex: hex is read in either case",
        &pk,
        msg,
        &sig.to_uppercase(),
        true,
    );
    case(&mut ex, "the key in upper-case hex", &pk.to_uppercase(), msg, &sig, true);
    case(&mut ex, "one character of the message changed", &pk, &msg.replace("two", "twO"), &sig, false);
    case(&mut ex, "the message without its last newline", &pk, msg.trim_end(), &sig, false);
    case(&mut ex, "the message with a newline more", &pk, &format!("{msg}\n"), &sig, false);
    case(&mut ex, "another key", &hex::encode(other.sign_public()), msg, &sig, false);
    let theirs = hex::encode(other.signature(msg.as_bytes()));
    case(&mut ex, "another key's signature over the same message", &pk, msg, &theirs, false);
    case(&mut ex, "the signature cut to 63 bytes", &pk, msg, &sig[..126], false);
    case(&mut ex, "the signature with a byte more", &pk, msg, &format!("{sig}00"), false);
    case(&mut ex, "an odd number of hex digits", &pk, msg, &sig[..127], false);
    case(&mut ex, "a signature that isn't hex", &pk, msg, &format!("zz{}", &sig[2..]), false);
    case(&mut ex, "no signature", &pk, msg, "", false);
    case(&mut ex, "a key of 31 bytes", &pk[..62], msg, &sig, false);
    case(&mut ex, "no key", "", msg, &sig, false);
    // The first byte-repeated 32 bytes that aren't a point on the curve.
    let not_a_point = (0u8..=255).map(|b| [b; 32]).find(|b| VerifyingKey::from_bytes(b).is_err()).unwrap();
    case(&mut ex, "a key that isn't a point on the curve", &hex::encode(not_a_point), msg, &sig, false);
    // A small-order key: the identity, with R the identity and s zero, passes
    // a lax check for any message; a strict one refuses a weak key.
    let identity = {
        let mut b = [0u8; 32];
        b[0] = 1;
        b
    };
    let forged = hex::encode([identity, [0; 32]].concat());
    let lax = VerifyingKey::from_bytes(&identity).unwrap().verify(
        msg.as_bytes(),
        &ed25519_dalek::Signature::from_bytes(&hex::decode(&forged).unwrap().try_into().unwrap()),
    );
    assert!(lax.is_ok(), "a lax check would pass this one");
    case(
        &mut ex,
        "the identity as the key (small order), with R the identity and s zero: a lax check passes it for any \
         message; refused",
        &hex::encode(identity),
        msg,
        &forged,
        false,
    );
    // s + L: the same signature, malleated, isn't canonical.
    let malleated = {
        let mut b: [u8; 64] = hex::decode(&sig).unwrap().try_into().unwrap();
        let l: [u8; 32] = [
            0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
        ];
        let mut carry = 0u16;
        for i in 0..32 {
            let s = u16::from(b[32 + i]) + u16::from(l[i]) + carry;
            b[32 + i] = s as u8;
            carry = s >> 8;
        }
        assert_eq!(carry, 0);
        hex::encode(b)
    };
    case(&mut ex, "the signature malleated (s + L, so s isn't canonical)", &pk, msg, &malleated, false);
    ex
}

// ---- kinds ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KindIn {
    kind: String,
}

fn run_kinds(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: KindIn = input(v)?;
    let k = parse_kind(&i.kind)?;
    let certifies: Vec<&str> = KINDS.into_iter().filter(|o| k.certifies(*o)).map(kind_str).collect();
    Ok(json!({ "approves": k.approves(), "certifies": certifies, "roots": k.roots(), "connects": k.connects() }))
}

fn kinds() -> Example {
    let mut ex = Example::new(
        "cert",
        "kinds",
        run_kinds,
        "What each kind of device may do: approve (sign others' certificates, revocations and team papers), which \
         kinds it certifies, whether it may be an account's first device (the root, self-signed), and whether it \
         opens channels to daemons. A CLI's and the desktop app's keys may be a program's on their computer, so \
         neither makes a browser or a recovery code; a machine approves nothing.",
        &[
            "arugula_e2e::Kind::approves",
            "arugula_e2e::Kind::certifies",
            "arugula_e2e::Kind::roots",
            "arugula_e2e::Kind::connects",
        ],
    )
    .shapes(
        "kind: browser, cli, daemon, recovery or app",
        "approves, roots, connects: booleans; certifies: the kinds it may sign a certificate of, in the order \
         browser, cli, daemon, recovery, app",
    );
    let all = ["browser", "cli", "daemon", "recovery", "app"];
    let table = [
        (Kind::Browser, true, &all[..], true, true),
        (Kind::Cli, true, &["cli", "daemon"][..], true, true),
        (Kind::Daemon, false, &[][..], false, false),
        (Kind::Recovery, true, &["browser", "cli", "recovery", "app"][..], false, false),
        (Kind::App, true, &["cli", "daemon", "app"][..], false, true),
    ];
    for (k, approves, certifies, roots, connects) in table {
        ex.case(
            &format!("a {} device", k.as_str()),
            json!({ "kind": k.as_str() }),
            json!({ "approves": approves, "certifies": certifies, "roots": roots, "connects": connects }),
        );
    }
    ex
}

// ---- the desktop app's devices ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppIn {
    kind: String,
    name: String,
}

fn run_app(_: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: AppIn = input(v)?;
    let mut c = Cert::new(&keys(1), ACCT, parse_kind(&i.kind)?, &i.name);
    c.created = 0;
    Ok(json!({ "from_app": c.from_app(), "is_browser": c.is_browser() }))
}

fn app_names() -> Example {
    let mut ex = Example::new(
        "cert",
        "app_names",
        run_app,
        "Which certificates are the desktop app's: an app one, or a browser one from before the app had a kind of \
         its own, which the app named \"Arugula app ...\" (\"illogical app ...\" before the rename), in any case. \
         Those don't approve browsers or deploys. A browser's or a phone's own device is a browser certificate \
         that isn't the app's.",
        &["arugula_e2e::Cert::from_app", "arugula_e2e::Cert::is_browser"],
    )
    .shapes("kind: the certificate's kind; name: its name", "from_app, is_browser: booleans");
    for (about, kind, name, from_app, is_browser) in [
        ("an app certificate, whatever its name", "app", "Chrome on Mac", true, false),
        ("a browser certificate the app named", "browser", "Arugula app on jake-air", true, false),
        ("one the app named in another case", "browser", "ARUGULA APP (AppImage) on geek", true, false),
        ("one the app named before the rename", "browser", "illogical app on geek", true, false),
        ("a browser's own", "browser", "Safari on iPhone", false, true),
        ("a browser whose name has the words later on", "browser", "Chrome, not the Arugula app", false, true),
        ("a CLI named like the app: only a browser's name counts", "cli", "Arugula app", false, false),
        ("a machine", "daemon", "geek", false, false),
        ("a recovery code", "recovery", "recovery 1", false, false),
    ] {
        ex.case(
            about,
            json!({ "kind": kind, "name": name }),
            json!({ "from_app": from_app, "is_browser": is_browser }),
        );
    }
    ex
}

// ---- bodies ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyIn {
    #[serde(default)]
    cert: Option<String>,
    #[serde(default)]
    revocation: Option<String>,
    #[serde(default)]
    join_proof_at: Option<u64>,
}

fn run_bodies(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: BodyIn = input(v)?;
    let body = match (i.cert, i.revocation, i.join_proof_at) {
        (Some(c), None, None) => ctx.get::<Cert>(&c)?.body(),
        (Some(c), None, Some(ms)) => join_proof_body(&ctx.get::<Cert>(&c)?, ms),
        (None, Some(r), None) => ctx.get::<Revocation>(&r)?.body(),
        _ => anyhow::bail!("one of cert or revocation"),
    };
    Ok(json!({ "body": body }))
}

fn bodies() -> Example {
    let mut ex = Example::new(
        "cert",
        "bodies",
        run_bodies,
        "The exact bytes a certificate's and a revocation's signature is over, and what a daemon signs when it \
         asks to join. Each first line is a domain of its own, frozen (#504: the rename to Arugula kept \
         \"illogical\"). A certificate's sig is its approver's signature over its body; a revocation's, its by's.",
        &["arugula_e2e::Cert::body", "arugula_e2e::Revocation::body", "arugula_e2e::cert::join_proof_body"],
    )
    .shapes(
        "cert: a certificate's label (with join_proof_at: the ms the daemon signs at, for its join proof), or \
         revocation: a revocation's label",
        "body: the signed text",
    );
    let root = ex.key("laptop", 1);
    let machine = ex.key("geek", 2);
    let rc = cert(&root, ACCT, Kind::Browser, "laptop", 1_000, &root);
    let dc = cert(&machine, ACCT, Kind::Daemon, "geek", 2_000, &root);
    let r = revoke(ACCT, &dc.device, 3_000, &root);
    for (l, c) in [("laptop", &rc), ("geek", &dc)] {
        ex.object(l, c);
    }
    ex.object("geek-revoked", &r);
    ex.computed("the root's certificate (self-signed: its approver is itself)", json!({ "cert": "laptop" }));
    ex.computed("a machine's, approved by the root", json!({ "cert": "geek" }));
    ex.computed("the machine taken off the account", json!({ "revocation": "geek-revoked" }));
    ex.computed("the machine's join proof at 2500 ms", json!({ "cert": "geek", "join_proof_at": 2_500 }));
    ex
}

// ---- check_form ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormIn {
    cert: String,
}

fn run_form(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: FormIn = input(v)?;
    Ok(json!({ "ok": ctx.get::<Cert>(&i.cert)?.check_form().is_ok() }))
}

fn check_form() -> Example {
    let mut ex = Example::new(
        "cert",
        "check_form",
        run_form,
        "Whether a certificate is well formed, which says nothing of its signature: version 1; account and \
         approver 1 to 64 of a-z, A-Z, 0-9, - and _; a name of 1 to 64 characters (not bytes), none a control \
         character; noise and sign 32 bytes of hex each; and a device id that is \
         hex(SHA-256(\"illogical device id\" || noise || sign))[..16], so control can't give one device another's \
         id. Trust::evaluate ignores a certificate that isn't.",
        &["arugula_e2e::Cert::check_form", "arugula_e2e::keys::device_id"],
    )
    .shapes("cert: a certificate's label", "ok: whether it's well formed");
    let k = ex.key("laptop", 1);
    let other = ex.key("other", 2);
    let good = cert(&k, ACCT, Kind::Browser, "laptop", 1_000, &k);
    let mut variants: Vec<(&str, &str, Cert, bool)> = vec![("good", "a root's certificate", good.clone(), true)];
    let with = |f: &dyn Fn(&mut Cert)| {
        let mut c = good.clone();
        f(&mut c);
        c
    };
    variants.extend([
        ("v2", "version 2", with(&|c| c.v = 2), false),
        ("account-space", "an account with a space", with(&|c| c.account = "ac ct".into()), false),
        ("account-empty", "no account", with(&|c| c.account = String::new()), false),
        ("account-64", "an account of 64 characters", with(&|c| c.account = "a".repeat(64)), true),
        ("account-65", "an account of 65 characters", with(&|c| c.account = "a".repeat(65)), false),
        ("account-dot", "an account with a dot", with(&|c| c.account = "a.b".into()), false),
        ("approver-empty", "no approver", with(&|c| c.approver = String::new()), false),
        ("name-empty", "no name", with(&|c| c.name = String::new()), false),
        ("name-newline", "a newline in the name", with(&|c| c.name = "lap\ntop".into()), false),
        ("name-tab", "a tab in the name", with(&|c| c.name = "lap\ttop".into()), false),
        ("name-64-chars", "a name of 64 characters, 128 bytes", with(&|c| c.name = "é".repeat(64)), true),
        ("name-65-chars", "a name of 65 characters", with(&|c| c.name = "a".repeat(65)), false),
        ("name-space", "a space in the name", with(&|c| c.name = "my laptop".into()), true),
        ("noise-short", "a noise key of 31 bytes", with(&|c| c.noise = "01".repeat(31)), false),
        ("noise-not-hex", "a noise key that isn't hex", with(&|c| c.noise = "zz".repeat(32)), false),
        (
            "sign-other",
            "another device's signing key, under this id",
            with(&|c| c.sign = hex::encode(other.sign_public())),
            false,
        ),
        ("device-other", "another device's id", with(&|c| c.device = other.id()), false),
        ("device-upper", "the id in upper case", with(&|c| c.device = c.device.to_uppercase()), false),
        ("sig-empty", "no signature: the form says nothing of it", with(&|c| c.sig = String::new()), true),
    ]);
    for (label, about, c, ok) in variants {
        ex.object(label, &c);
        ex.case(about, json!({ "cert": label }), json!({ "ok": ok }));
    }
    ex
}

// ---- evaluate ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustIn {
    account: String,
    root: String,
    certs: Vec<String>,
    #[serde(default)]
    revocations: Vec<String>,
}

fn trusted_json(t: &Trusted) -> Value {
    let mut v: Vec<&Cert> = t.devices.values().collect();
    v.sort_by(|a, b| a.device.cmp(&b.device));
    json!(v.iter().map(|c| json!({ "device": c.device, "name": c.name, "approver": c.approver })).collect::<Vec<_>>())
}

fn run_evaluate(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: TrustIn = input(v)?;
    let t = Trust { account: i.account, root: i.root };
    let trusted = t.evaluate(&ctx.all::<Cert>(&i.certs)?, &ctx.all::<Revocation>(&i.revocations)?);
    Ok(json!({ "trusted": trusted_json(&trusted) }))
}

/// An account's devices, for evaluate and refusal.
struct World {
    ex: Example,
}

impl World {
    fn k(&mut self, label: &str, n: u8) -> arugula_e2e::DeviceKeys {
        self.ex.key(label, n)
    }

    fn c(&mut self, label: &str, c: &Cert) -> String {
        self.ex.object(label, c)
    }

    fn r(&mut self, label: &str, r: &Revocation) -> String {
        self.ex.object(label, r)
    }
}

fn evaluate() -> Example {
    let ex = Example::new(
        "cert",
        "evaluate",
        run_evaluate,
        "Which of the certificates control hands out an account trusts now, from the root device a daemon pinned: \
         each must chain back to that root (self-signed, of a kind that roots), every link signed by a device \
         valid when the link was made, of a kind that certifies the next. A certificate not signed by the key of \
         the device it names as approver is set aside before anything else. A recovery code lets in one device: \
         the first it signed, by created time, then by device id. A revocation counts when its signer is \
         valid then (checked twice, so a revoked device's own revocations stop counting); a browser or a recovery \
         code is taken off only by a device that's a browser all the way to the root, along certificates that \
         count (each a browser's, or a recovery code's above one). A device revoked at T no \
         longer counts, nor what it approved after T; what it approved before T stays. Certificates don't expire: \
         time matters only against revocations. Anything that doesn't check out is ignored, never an error: \
         control may send junk, and it can only shrink the set.",
        &["arugula_e2e::Trust::evaluate"],
    )
    .shapes(
        "account and root: what's pinned (the account, its root device's id); certs and revocations: labels of \
         objects, in the order control sent them",
        "trusted: the devices trusted now, sorted by id: each one's device id, name and approver (which of its \
         certificates counted)",
    );
    let mut w = World { ex };
    let root = w.k("laptop", 1);
    let phone = w.k("phone", 2);
    let tablet = w.k("tablet", 3);
    let geek = w.k("geek", 4);
    let cli = w.k("terminal", 5);
    let app = w.k("app", 6);
    let code = w.k("recovery", 7);
    let newlaptop = w.k("new-laptop", 8);
    let fake = w.k("control-root", 10);
    let sneaky = w.k("control-device", 11);
    let rid = root.id();
    let put = |w: &mut World, label: &str, cert: Cert| w.c(label, &cert);
    put(&mut w, "laptop", ac(&root, Kind::Browser, "laptop", 1_000, &root));
    put(&mut w, "phone", ac(&phone, Kind::Browser, "phone", 1_100, &root));
    put(&mut w, "tablet", ac(&tablet, Kind::Browser, "tablet", 1_200, &root));
    put(&mut w, "geek", ac(&geek, Kind::Daemon, "geek", 1_300, &phone));
    put(&mut w, "terminal", ac(&cli, Kind::Cli, "terminal", 1_400, &root));
    put(&mut w, "app", ac(&app, Kind::App, "Arugula app on box", 1_500, &root));
    put(&mut w, "recovery", ac(&code, Kind::Recovery, "recovery 1", 1_600, &root));
    put(&mut w, "new-laptop", ac(&newlaptop, Kind::Browser, "new laptop", 1_700, &code));
    // Control's own root, and a device under it.
    put(&mut w, "control-root", ac(&fake, Kind::Browser, "laptop", 1_000, &fake));
    put(&mut w, "control-device", ac(&sneaky, Kind::Browser, "control", 1_800, &fake));
    // A device control says the real root approved, signed with its own key.
    let mut claims = ac(&sneaky, Kind::Browser, "control", 1_800, &fake);
    claims.approver = rid.clone();
    put(&mut w, "control-device-claims-root", claims);
    // The phone's certificate, renamed after it was signed.
    let mut renamed = ac(&phone, Kind::Browser, "phone", 1_100, &root);
    renamed.name = "laptop".into();
    put(&mut w, "phone-renamed", renamed);
    let mut elsewhere = ac(&phone, Kind::Browser, "phone", 1_100, &root);
    elsewhere.account = "elsewhere".into();
    elsewhere.sign_with(&root);
    put(&mut w, "phone-other-account", elsewhere);
    // What a CLI, the app, a recovery code and a machine sign.
    let mut made = vec![];
    for (by_label, by, by_n) in
        [("terminal", &cli, 20u8), ("app", &app, 30), ("recovery", &code, 40), ("geek", &geek, 50)]
    {
        for (i, kind) in KINDS.into_iter().enumerate() {
            let n = by_n + i as u8;
            let label = format!("{}-by-{by_label}", kind.as_str());
            let k = w.k(&label, n);
            let cc = ac(&k, kind, &format!("{} by {by_label}", kind.as_str()), 2_000, by);
            put(&mut w, &label, cc);
            made.push((by_label, kind, label));
        }
    }
    // Roots of other kinds.
    let approot = w.k("app-root", 60);
    put(&mut w, "app-root", ac(&approot, Kind::App, "Arugula app", 1_000, &approot));
    let cliroot = w.k("cli-root", 61);
    put(&mut w, "cli-root", ac(&cliroot, Kind::Cli, "terminal", 1_000, &cliroot));
    let daemonroot = w.k("daemon-root", 62);
    put(&mut w, "daemon-root", ac(&daemonroot, Kind::Daemon, "geek", 1_000, &daemonroot));
    // Revocations: the phone at 20_000, with what it approved before and after.
    let before = w.k("before", 70);
    let after = w.k("after", 71);
    put(&mut w, "before", ac(&before, Kind::Daemon, "before", 10_000, &phone));
    put(&mut w, "after", ac(&after, Kind::Daemon, "after", 30_000, &phone));
    let pid = phone.id();
    w.r("phone-revoked-20000", &revoke(ACCT, &pid, 20_000, &root));
    w.r("phone-revoked-5000", &revoke(ACCT, &pid, 5_000, &root));
    w.r("phone-revoked-50000", &revoke(ACCT, &pid, 50_000, &root));
    // The revoked phone revoking the tablet out of spite.
    w.r("tablet-revoked-by-phone", &revoke(ACCT, &tablet.id(), 40_000, &phone));
    // A CLI and the app revoking a browser, the root, a machine and a CLI.
    for (by_label, by) in [("terminal", &cli), ("app", &app)] {
        for (what, id) in [("phone", pid.clone()), ("laptop", rid.clone()), ("geek", geek.id())] {
            w.r(&format!("{what}-revoked-by-{by_label}"), &revoke(ACCT, &id, 40_000, by));
        }
    }
    let cli2 = w.k("terminal-2", 72);
    put(&mut w, "terminal-2", ac(&cli2, Kind::Cli, "terminal 2", 1_450, &root));
    w.r("terminal-2-revoked-by-terminal", &revoke(ACCT, &cli2.id(), 40_000, &cli));
    // Revocations that don't check out.
    let mut bad_sig = revoke(ACCT, &pid, 40_000, &root);
    bad_sig.at = 40_001;
    w.r("phone-revoked-bad-signature", &bad_sig);
    let mut v2 = revoke(ACCT, &pid, 40_000, &root);
    v2.v = 2;
    w.r("phone-revoked-v2", &v2);
    let mut other_acct = revoke(ACCT, &pid, 40_000, &root);
    other_acct.account = "elsewhere".into();
    other_acct.sig = hex::encode(root.signature(other_acct.body().as_bytes()));
    w.r("phone-revoked-in-another-account", &other_acct);
    w.r("phone-revoked-by-control", &revoke(ACCT, &pid, 40_000, &fake));
    // The recovery code revoked backdated (before the browser it let in).
    for (by_label, by) in [("terminal", &cli), ("app", &app), ("laptop", &root)] {
        w.r(&format!("recovery-revoked-at-1-by-{by_label}"), &revoke(ACCT, &code.id(), 1, by));
    }
    w.r("phone-revoked-by-new-laptop", &revoke(ACCT, &pid, 40_000, &newlaptop));
    // Another certificate for the new laptop's keys, as a CLI: unsigned,
    // and signed by the phone (before it's revoked).
    let mut unsigned = ac(&newlaptop, Kind::Cli, "new laptop", 1_700, &code);
    unsigned.sig = String::new();
    put(&mut w, "new-laptop-as-cli-unsigned", unsigned);
    put(&mut w, "new-laptop-as-cli-by-phone", ac(&newlaptop, Kind::Cli, "new laptop", 1_700, &phone));
    // A long chain: 65 browsers below the root, each approving the next
    // (chain-i's key is byte 100 + i; not listed in keys).
    let mut chain = vec!["laptop".to_owned()];
    let mut prev = keys(1);
    for i in 0..65u8 {
        let label = format!("chain-{i}");
        let k = keys(100 + i);
        put(&mut w, &label, ac(&k, Kind::Browser, &label, 3_000 + u64::from(i), &prev));
        chain.push(label);
        prev = k;
    }
    // Two devices approving each other, neither the root.
    let (ka, kb) = (w.k("loop-a", 180), w.k("loop-b", 181));
    put(&mut w, "loop-a", ac(&ka, Kind::Browser, "loop a", 2_000, &kb));
    put(&mut w, "loop-b", ac(&kb, Kind::Browser, "loop b", 2_000, &ka));

    let mut ex = w.ex;
    let case = |ex: &mut Example, about: &str, root: &str, certs: &[&str], revs: &[&str], want: &[&str]| {
        let input = json!({ "account": ACCT, "root": root, "certs": labels(certs), "revocations": labels(revs) });
        let got = ex.computed(about, input);
        let ids: Vec<String> =
            got["trusted"].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap().to_owned()).collect();
        let mut want: Vec<String> = want.iter().map(|s| (*s).to_owned()).collect();
        let mut ids_sorted = ids.clone();
        ids_sorted.sort();
        want.sort();
        assert_eq!(ids_sorted, want, "cert/evaluate: {about}");
    };
    let base = ["laptop", "phone", "tablet", "geek"];
    case(&mut ex, "the root alone", &rid, &["laptop"], &[], &["laptop"]);
    case(
        &mut ex,
        "a chain: the root, a phone it approved, a machine the phone approved",
        &rid,
        &["laptop", "phone", "geek"],
        &[],
        &["laptop", "phone", "geek"],
    );
    case(&mut ex, "a machine whose approver control left out", &rid, &["laptop", "geek"], &[], &["laptop"]);
    case(
        &mut ex,
        "control's own root and a device under it: not counted",
        &rid,
        &["laptop", "control-root", "control-device"],
        &[],
        &["laptop"],
    );
    case(
        &mut ex,
        "a device control says the root approved, signed with another key",
        &rid,
        &["laptop", "control-device-claims-root"],
        &[],
        &["laptop"],
    );
    case(&mut ex, "a certificate renamed after it was signed", &rid, &["laptop", "phone-renamed"], &[], &["laptop"]);
    case(
        &mut ex,
        "the renamed copy beside the real one: the real one counts",
        &rid,
        &["laptop", "phone-renamed", "phone"],
        &[],
        &["laptop", "phone"],
    );
    case(&mut ex, "another account's certificate", &rid, &["laptop", "phone-other-account"], &[], &["laptop"]);
    case(
        &mut ex,
        "the wrong root pinned (control's): only its own chain",
        &fake.id(),
        &["laptop", "phone", "control-root", "control-device"],
        &[],
        &["laptop", "control"],
    );
    case(&mut ex, "a root pinned that isn't self-signed (the tablet): nothing", &tablet.id(), &base, &[], &[]);
    case(&mut ex, "a root pinned that control never sent: nothing", "0000000000000000", &base, &[], &[]);
    case(&mut ex, "the app as a root: it can't be one", &approot.id(), &["app-root"], &[], &[]);
    case(&mut ex, "a machine as a root: it can't be one", &daemonroot.id(), &["daemon-root"], &[], &[]);
    case(&mut ex, "a CLI as a root: it may be", &cliroot.id(), &["cli-root"], &[], &["terminal"]);
    for (by_label, ok) in [
        ("terminal", &["cli", "daemon"][..]),
        ("app", &["cli", "daemon", "app"][..]),
        ("recovery", &["browser", "cli", "recovery", "app"][..]),
        ("geek", &[][..]),
    ] {
        let mine: Vec<&(&str, Kind, String)> = made.iter().filter(|(b, _, _)| b == &by_label).collect();
        // A recovery code lets in one device, so what it approves is
        // asked one at a time.
        let groups: Vec<Vec<&(&str, Kind, String)>> =
            if by_label == "recovery" { mine.iter().map(|m| vec![*m]).collect() } else { vec![mine] };
        for group in groups {
            let mut certs: Vec<String> =
                ["laptop", "phone", "geek", "terminal", "app", "recovery"].map(String::from).to_vec();
            let mut want: Vec<String> =
                ["laptop", "phone", "geek", "terminal", "Arugula app on box", "recovery 1"].map(String::from).to_vec();
            for (_, kind, label) in &group {
                certs.push(label.clone());
                if ok.contains(&kind.as_str()) {
                    want.push(format!("{} by {by_label}", kind.as_str()));
                }
            }
            let certs: Vec<&str> = certs.iter().map(String::as_str).collect();
            let want: Vec<&str> = want.iter().map(String::as_str).collect();
            let what = match by_label {
                "terminal" => "what a CLI approves: CLIs and machines, never a browser or a recovery code".to_owned(),
                "app" => {
                    "what the app approves: apps, CLIs and machines, never a browser or a recovery code".to_owned()
                }
                "recovery" => format!(
                    "a recovery code approving a {}: people's devices, not machines",
                    group.first().map(|(_, k, _)| k.as_str()).unwrap_or_default()
                ),
                _ => "what a machine approves: nothing".to_owned(),
            };
            case(&mut ex, &what, &rid, &certs, &[], &want);
        }
    }
    let mut every: Vec<&str> = ["laptop", "phone", "geek", "terminal", "app", "recovery"].to_vec();
    let mut want = vec!["laptop", "phone", "geek", "terminal", "Arugula app on box", "recovery 1"];
    every.extend(made.iter().filter(|(b, _, _)| *b == "recovery").map(|(_, _, l)| l.as_str()));
    // All made at the same time: the first by id counts.
    let first = made
        .iter()
        .filter(|(b, k, _)| *b == "recovery" && Kind::Recovery.certifies(*k))
        .min_by_key(|(_, k, _)| keys(40 + KINDS.iter().position(|x| x == k).unwrap_or_default() as u8).id())
        .map(|(_, k, _)| format!("{} by recovery", k.as_str()))
        .unwrap();
    want.push(&first);
    case(
        &mut ex,
        "a recovery code lets in one device: the first it signed (by time, then id)",
        &rid,
        &every,
        &[],
        &want,
    );
    let timed = ["laptop", "phone", "before", "after"];
    case(
        &mut ex,
        "the phone revoked at 20000: what it approved at 10000 stays, at 30000 goes",
        &rid,
        &timed,
        &["phone-revoked-20000"],
        &["laptop", "before"],
    );
    case(&mut ex, "revoked at 5000: both go", &rid, &timed, &["phone-revoked-5000"], &["laptop"]);
    case(
        &mut ex,
        "revoked at 50000: both stay, the phone goes",
        &rid,
        &timed,
        &["phone-revoked-50000"],
        &["laptop", "before", "after"],
    );
    case(
        &mut ex,
        "revoked twice, at 50000 and 5000: the earlier counts",
        &rid,
        &timed,
        &["phone-revoked-50000", "phone-revoked-5000"],
        &["laptop"],
    );
    case(
        &mut ex,
        "the revoked phone revoking the tablet: a revoked device's revocations don't count",
        &rid,
        &["laptop", "phone", "tablet"],
        &["phone-revoked-5000", "tablet-revoked-by-phone"],
        &["laptop", "tablet"],
    );
    let six = ["laptop", "phone", "geek", "terminal", "app", "terminal-2"];
    let six_names = ["laptop", "phone", "geek", "terminal", "Arugula app on box", "terminal 2"];
    for by in ["terminal", "app"] {
        let who = if by == "terminal" { "a CLI" } else { "the app" };
        let without = |gone: &str| six_names.iter().copied().filter(|n| *n != gone).collect::<Vec<_>>();
        case(
            &mut ex,
            &format!("{who} revoking the phone: doesn't count"),
            &rid,
            &six,
            &[&format!("phone-revoked-by-{by}")],
            &six_names,
        );
        case(
            &mut ex,
            &format!("{who} revoking the root: doesn't count"),
            &rid,
            &six,
            &[&format!("laptop-revoked-by-{by}")],
            &six_names,
        );
        case(
            &mut ex,
            &format!("{who} revoking a machine: counts"),
            &rid,
            &six,
            &[&format!("geek-revoked-by-{by}")],
            &without("geek"),
        );
    }
    case(
        &mut ex,
        "a CLI revoking another CLI: counts",
        &rid,
        &six,
        &["terminal-2-revoked-by-terminal"],
        &["laptop", "phone", "geek", "terminal", "Arugula app on box"],
    );
    case(
        &mut ex,
        "a revocation whose signature doesn't check: ignored",
        &rid,
        &base,
        &["phone-revoked-bad-signature"],
        &["laptop", "phone", "tablet", "geek"],
    );
    case(
        &mut ex,
        "a revocation of version 2: ignored",
        &rid,
        &base,
        &["phone-revoked-v2"],
        &["laptop", "phone", "tablet", "geek"],
    );
    case(
        &mut ex,
        "a revocation in another account: ignored",
        &rid,
        &base,
        &["phone-revoked-in-another-account"],
        &["laptop", "phone", "tablet", "geek"],
    );
    case(
        &mut ex,
        "a revocation control signed: ignored",
        &rid,
        &["laptop", "phone", "control-root"],
        &["phone-revoked-by-control"],
        &["laptop", "phone"],
    );
    let rec = ["laptop", "terminal", "app", "recovery", "new-laptop", "phone"];
    let rec_all = ["laptop", "terminal", "Arugula app on box", "recovery 1", "new laptop", "phone"];
    case(&mut ex, "a browser a recovery code let in", &rid, &rec, &[], &rec_all);
    case(
        &mut ex,
        "the recovered browser revokes the lost phone",
        &rid,
        &rec,
        &["phone-revoked-by-new-laptop"],
        &["laptop", "terminal", "Arugula app on box", "recovery 1", "new laptop"],
    );
    for (extra, what) in [("new-laptop-as-cli-unsigned", "unsigned"), ("new-laptop-as-cli-by-phone", "signed as a CLI")]
    {
        let mut certs = vec![extra];
        certs.extend(rec);
        case(
            &mut ex,
            &format!(
                "the recovered browser revokes the lost phone, with another certificate for it first ({what}): \
                 the revocation stands on the chain that counts"
            ),
            &rid,
            &certs,
            &["phone-revoked-by-new-laptop"],
            &["laptop", "terminal", "Arugula app on box", "recovery 1", "new laptop"],
        );
    }
    case(
        &mut ex,
        "the recovery code revoked backdated by a CLI: doesn't count",
        &rid,
        &rec,
        &["recovery-revoked-at-1-by-terminal"],
        &rec_all,
    );
    case(
        &mut ex,
        "the recovery code revoked backdated by the app: doesn't count",
        &rid,
        &rec,
        &["recovery-revoked-at-1-by-app"],
        &rec_all,
    );
    case(
        &mut ex,
        "the recovery code revoked backdated by the root: it and the browser it let in go",
        &rid,
        &rec,
        &["recovery-revoked-at-1-by-laptop"],
        &["laptop", "terminal", "Arugula app on box", "phone"],
    );
    let chain_refs: Vec<&str> = chain.iter().map(String::as_str).collect();
    let deep: Vec<String> = (0..64).map(|i| format!("chain-{i}")).chain(["laptop".to_owned()]).collect();
    let deep: Vec<&str> = deep.iter().map(String::as_str).collect();
    case(
        &mut ex,
        "a chain of 65 browsers below the root: 64 links deep is as far as it goes (chain-64 is refused)",
        &rid,
        &chain_refs,
        &[],
        &deep,
    );
    case(
        &mut ex,
        "two devices approving each other, neither the root",
        &rid,
        &["laptop", "loop-a", "loop-b"],
        &[],
        &["laptop"],
    );
    ex
}

// ---- refusal ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefusalIn {
    account: String,
    root: String,
    certs: Vec<String>,
    #[serde(default)]
    revocations: Vec<String>,
    cert: String,
}

fn run_refusal(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: RefusalIn = input(v)?;
    let t = Trust { account: i.account, root: i.root };
    let r = t.refusal(&ctx.all::<Cert>(&i.certs)?, &ctx.all::<Revocation>(&i.revocations)?, &ctx.get(&i.cert)?);
    Ok(json!({ "refusal": r.map(|r| r.code()) }))
}

fn refusal() -> Example {
    let mut ex = Example::new(
        "cert",
        "refusal",
        run_refusal,
        "Why a new certificate wouldn't count beside the account's (#327), which control says before it stores \
         one, so a client can say which check failed: revoked (its device was taken off: it needs a new key), \
         approver_untrusted, bad_signature, cant_approve (a machine approving), recovery_for_machine, needs_browser \
         (a CLI or the app approving a browser, a phone or a recovery code), recovery_spent (a recovery code that \
         let a device in already: each lets in one), no_chain (anything else: its form, \
         its kind, its time). None when it counts.",
        &["arugula_e2e::Trust::refusal", "arugula_e2e::Refusal::code"],
    )
    .shapes(
        "account, root: what's pinned; certs, revocations: the account's, by label; cert: the new one, by label",
        "refusal: the failed check's code, or null when it counts",
    );
    let root = ex.key("laptop", 1);
    let code = ex.key("recovery", 2);
    let machine = ex.key("box", 3);
    let cli = ex.key("terminal", 4);
    let app = ex.key("app", 5);
    let stranger = ex.key("stranger", 6);
    let fresh = ex.key("new", 7);
    let rid = root.id();
    for (l, cc) in [
        ("laptop", ac(&root, Kind::Browser, "laptop", 1_000, &root)),
        ("recovery", ac(&code, Kind::Recovery, "recovery 1", 1_100, &root)),
        ("box", ac(&machine, Kind::Daemon, "box", 1_200, &root)),
        ("terminal", ac(&cli, Kind::Cli, "terminal", 1_300, &root)),
        ("app", ac(&app, Kind::App, "Arugula app on box", 1_400, &root)),
    ] {
        ex.object(l, &cc);
    }
    ex.object("box-revoked", &revoke(ACCT, &machine.id(), 5_000, &root));
    let have = ["laptop", "recovery", "box", "terminal", "app"];
    let new = |ex: &mut Example, label: &str, kind: Kind, by: &arugula_e2e::DeviceKeys| {
        ex.object(label, &ac(&fresh, kind, "new", 2_000, by))
    };
    let mut forged = ac(&fresh, Kind::Browser, "new", 2_000, &root);
    forged.name = "renamed".into();
    ex.object("browser-renamed-after-signing", &forged);
    let mut elsewhere = ac(&fresh, Kind::Browser, "new", 2_000, &root);
    elsewhere.account = "elsewhere".into();
    elsewhere.sign_with(&root);
    ex.object("browser-in-another-account", &elsewhere);
    ex.object("box-again", &ac(&machine, Kind::Daemon, "box", 6_000, &root));
    let mut cases: Vec<(String, String, Option<&str>)> = vec![];
    let mut add = |ex: &mut Example, label: &str, kind: Kind, by: &arugula_e2e::DeviceKeys, about: &str, want| {
        new(ex, label, kind, by);
        cases.push((about.to_owned(), label.to_owned(), want));
    };
    add(&mut ex, "browser-by-root", Kind::Browser, &root, "a browser the root approved: it counts", None);
    add(
        &mut ex,
        "browser-by-stranger",
        Kind::Browser,
        &stranger,
        "approved by a device the account doesn't have",
        Some("approver_untrusted"),
    );
    add(&mut ex, "browser-by-box", Kind::Browser, &machine, "approved by a machine", Some("cant_approve"));
    add(
        &mut ex,
        "daemon-by-recovery",
        Kind::Daemon,
        &code,
        "a machine a recovery code approved",
        Some("recovery_for_machine"),
    );
    add(&mut ex, "browser-by-recovery", Kind::Browser, &code, "a browser a recovery code approved: it counts", None);
    for (by_label, by) in [("terminal", &cli), ("app", &app)] {
        for kind in KINDS {
            let ok = match by_label {
                "terminal" => matches!(kind, Kind::Cli | Kind::Daemon),
                _ => matches!(kind, Kind::Cli | Kind::Daemon | Kind::App),
            };
            let who = if by_label == "terminal" { "a CLI" } else { "the app" };
            let about = format!("a {} {who} approved", kind.as_str());
            add(
                &mut ex,
                &format!("{}-by-{by_label}", kind.as_str()),
                kind,
                by,
                &about,
                (!ok).then_some("needs_browser"),
            );
        }
    }
    for (about, label, want) in cases {
        ex.case(
            &about,
            json!({ "account": ACCT, "root": rid, "certs": labels(&have), "revocations": [], "cert": label }),
            json!({ "refusal": want }),
        );
    }
    let case = |ex: &mut Example, about: &str, new: &str, revs: &[&str], want: Option<&str>| {
        ex.case(
            about,
            json!({ "account": ACCT, "root": rid, "certs": labels(&have), "revocations": labels(revs), "cert": new }),
            json!({ "refusal": want }),
        );
    };
    case(&mut ex, "renamed after the root signed it", "browser-renamed-after-signing", &[], Some("bad_signature"));
    case(&mut ex, "in another account", "browser-in-another-account", &[], Some("no_chain"));
    case(&mut ex, "a removed machine approved again with its old key", "box-again", &["box-revoked"], Some("revoked"));
    // The recovery code let a browser in already (an earlier one).
    let used = ex.key("recovered", 8);
    ex.object("recovered", &ac(&used, Kind::Browser, "recovered", 1_500, &code));
    ex.case(
        "a browser a recovery code approved, after it let another in",
        json!({
            "account": ACCT, "root": rid, "certs": labels(&[&have[..], &["recovered"]].concat()),
            "revocations": [], "cert": "browser-by-recovery",
        }),
        json!({ "refusal": "recovery_spent" }),
    );
    ex
}

// ---- browser_chained ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChainedIn {
    account: String,
    root: String,
    certs: Vec<String>,
    device: String,
}

fn run_chained(ctx: &Ctx, v: &Value) -> anyhow::Result<Value> {
    let i: ChainedIn = input(v)?;
    let t = Trust { account: i.account, root: i.root };
    Ok(json!({ "ok": t.browser_chained(&ctx.all::<Cert>(&i.certs)?, &i.device) }))
}

fn browser_chained() -> Example {
    let mut ex = Example::new(
        "cert",
        "browser_chained",
        run_chained,
        "Whether a device is a browser all the way back to the root: what may make a team's owner or editor, or \
         take a browser or a phone off an account. The device itself must be a browser certificate; each link \
         before it a browser or a recovery code (the person's own, which only browsers make); the chain must end \
         at the pinned root, self-signed. Every certificate kept for an id on the way is looked at, not only the \
         one that made it trusted, so one more from control can only refuse. Signatures aren't checked here (ask \
         Trust::evaluate whether the device is trusted at all). The app's old browser certificates (named \
         \"Arugula app ...\") pass: control refuses their new papers by is_browser on every link.",
        &["arugula_e2e::Trust::browser_chained", "arugula_e2e::cert::browser_link"],
    )
    .shapes(
        "account, root: what's pinned; certs: the account's, by label; device: the device's id",
        "ok: whether it's a browser all the way to the root",
    );
    let root = ex.key("laptop", 1);
    let phone = ex.key("phone", 2);
    let cli = ex.key("terminal", 3);
    let laundered = ex.key("browser-by-terminal", 4);
    let code = ex.key("recovery", 5);
    let recovered = ex.key("new-laptop", 6);
    let machine = ex.key("box", 7);
    let app = ex.key("app", 8);
    let oldapp = ex.key("old-app", 9);
    let lone = ex.key("lone", 10);
    let all = [
        ("laptop", ac(&root, Kind::Browser, "laptop", 2_000, &root)),
        ("phone", ac(&phone, Kind::Browser, "phone", 2_000, &root)),
        ("terminal", ac(&cli, Kind::Cli, "terminal", 2_000, &root)),
        ("browser-by-terminal", ac(&laundered, Kind::Browser, "laundered", 2_000, &cli)),
        ("recovery", ac(&code, Kind::Recovery, "recovery 1", 2_000, &root)),
        ("new-laptop", ac(&recovered, Kind::Browser, "new laptop", 2_000, &code)),
        ("box", ac(&machine, Kind::Daemon, "box", 2_000, &phone)),
        ("app", ac(&app, Kind::App, "Arugula app", 2_000, &root)),
        ("old-app", ac(&oldapp, Kind::Browser, "Arugula app on jake-air", 2_000, &root)),
        ("lone", ac(&lone, Kind::Browser, "lone", 2_000, &lone)),
    ];
    for (l, cc) in &all {
        ex.object(l, cc);
    }
    // Control sends one more certificate for the phone: by the CLI.
    ex.object("phone-by-terminal", &ac(&phone, Kind::Browser, "phone", 2_000, &cli));
    let mut chain = vec!["laptop".to_owned()];
    let mut prev = keys(1);
    // chain-i's key is byte 100 + i (not listed in keys).
    for i in 0..64u8 {
        let label = format!("chain-{i}");
        let k = keys(100 + i);
        ex.object(&label, &ac(&k, Kind::Browser, &label, 2_000, &prev));
        chain.push(label);
        prev = k;
    }
    let everyone: Vec<&str> = all.iter().map(|(l, _)| *l).collect();
    let rid = root.id();
    let case = |ex: &mut Example, about: &str, certs: &[&str], device: &str, ok: bool| {
        ex.case(
            about,
            json!({ "account": ACCT, "root": rid, "certs": labels(certs), "device": device }),
            json!({ "ok": ok }),
        );
    };
    case(&mut ex, "the root itself", &everyone, &root.id(), true);
    case(&mut ex, "a phone the root approved", &everyone, &phone.id(), true);
    case(&mut ex, "a CLI", &everyone, &cli.id(), false);
    case(&mut ex, "a browser a CLI approved", &everyone, &laundered.id(), false);
    case(&mut ex, "a browser a recovery code let in", &everyone, &recovered.id(), true);
    case(&mut ex, "the recovery code itself: it doesn't stand for a browser", &everyone, &code.id(), false);
    case(&mut ex, "a machine a browser approved", &everyone, &machine.id(), false);
    case(&mut ex, "the app", &everyone, &app.id(), false);
    case(
        &mut ex,
        "the app's browser certificate from before it had a kind: passes here",
        &everyone,
        &oldapp.id(),
        true,
    );
    case(&mut ex, "a self-signed browser that isn't the root", &everyone, &lone.id(), false);
    case(&mut ex, "a device nobody sent", &everyone, "0000000000000000", false);
    let mut more = everyone.clone();
    more.push("phone-by-terminal");
    case(
        &mut ex,
        "the phone, with one more certificate for it from control, approved by the CLI",
        &more,
        &phone.id(),
        false,
    );
    let refs: Vec<&str> = chain.iter().map(String::as_str).collect();
    let id = |n: u8| keys(100 + n).id();
    case(&mut ex, "63 browsers down a chain from the root: 64 devices", &refs, &id(62), true);
    case(&mut ex, "64 down: 65 devices is longer than 64", &refs, &id(63), false);
    ex
}
