// S46 (#382) harness page: make a passkey on one origin, sign on another,
// and show what came back (flags, clientDataJSON's origin, whether the
// signature checks out against the published log).
import { b64u, flagsOf, fromCose, hex, sha256, unb64u, verify } from "/passkey-log.mjs";

const $ = (id) => document.getElementById(id);
const cfg = await (await fetch("/config.json")).json();
const results = []; // for local-chromium.mjs

// What keeps a passkey, from its AAGUID (passkey.rs's `provider`, plus a
// few from the community list).
const PROVIDERS = {
  fbfc3007154e4ecc8c0b6e020557d7bd: "iCloud Keychain",
  dd4ec289e01d41c9bb8970fa845d4bf2: "iCloud Keychain (managed)",
  ea9b8d664d011d213ce4b6b48cb575d4: "Google Password Manager",
  adce000235bcc60a648b0b25f1f05503: "Chrome on Mac",
  "08987058cadc4b81b6e130de50dcbe96": "Windows Hello",
  "9ddd1817af5a4672a2b93e3dd95000a9": "Windows Hello",
  "6028b017b1d44c02b4b3afcdafc96bb2": "Windows Hello",
  bada5566a7aa401fbd9645619a55120d: "1Password",
  d548826e79b4db40a3d811116f7e8349: "Bitwarden",
  "50726f74546563686e6f6c6f67792041": "Proton Pass",
  fdb141b25d84443e8a354698c205a502: "KeePassXC",
  "00000000000000000000000000000000": "(zeros: won't say)",
  "01020304050607080102030405060708": "(Chromium virtual authenticator)",
};

// ---------------------------------------------------------------- CBOR

function cbor(bytes) {
  let i = 0;
  const u = (n) => {
    let v = 0;
    for (let k = 0; k < n; k++) v = v * 256 + bytes[i++];
    return v;
  };
  const item = () => {
    const b = bytes[i++], major = b >> 5, info = b & 31;
    const len = info < 24 ? info : info === 24 ? u(1) : info === 25 ? u(2) : info === 26 ? u(4) : u(8);
    switch (major) {
      case 0: return len;
      case 1: return -1 - len;
      case 2: { const v = bytes.slice(i, i + len); i += len; return v; }
      case 3: { const v = new TextDecoder().decode(bytes.slice(i, i + len)); i += len; return v; }
      case 4: return Array.from({ length: len }, item);
      case 5: { const m = new Map(); for (let k = 0; k < len; k++) { const key = item(); m.set(key, item()); } return m; }
      case 7: return info === 20 ? false : info === 21 ? true : null;
      default: throw new Error(`CBOR major ${major}?`);
    }
  };
  const v = item();
  return { value: v, used: i };
}

// ---------------------------------------------------------------- page

$("role").textContent = cfg.role;
$("origin").textContent = location.origin;
$("rpid").textContent = cfg.rp;
if (cfg.parent) {
  $("parent-box").hidden = false;
  $("parentid").textContent = cfg.parent;
}
$("reg-rp").disabled = cfg.role !== "rp";

async function capabilities() {
  const c = { userAgent: navigator.userAgent, isSecureContext, PublicKeyCredential: typeof PublicKeyCredential };
  if (typeof PublicKeyCredential !== "undefined") {
    try { c.uvpaa = await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable(); } catch (e) { c.uvpaa = String(e); }
    try {
      c.clientCapabilities = PublicKeyCredential.getClientCapabilities
        ? await PublicKeyCredential.getClientCapabilities()
        : "getClientCapabilities missing";
    } catch (e) { c.clientCapabilities = String(e); }
  }
  try {
    const r = await fetch(`https://${cfg.rp}/.well-known/webauthn`);
    c.wellKnown = `${r.status} ${r.headers.get("content-type")} ${await r.text()}`;
  } catch (e) { c.wellKnown = String(e); }
  return c;
}
const caps = await capabilities();
$("caps").textContent = JSON.stringify(caps, null, 1);

function show(r) {
  results.push(r);
  window.__s46 = results;
  $("verdict").innerHTML = r.ok
    ? `<span class="ok">${r.step}: worked</span> in ${r.ms} ms${r.verified ? `; log check: ${r.verified}` : ""}`
    : `<span class="bad">${r.step}: ${r.error}</span> after ${r.ms} ms`;
  $("flags").innerHTML = r.flags
    ? Object.entries(r.flags).map(([k, v]) => `<span>${k}=${v ? 1 : 0}</span>`).join("")
    : "";
  $("detail").textContent = JSON.stringify(r, null, 1);
  const f = r.flags || {};
  const browser = navigator.userAgent.replace(/^Mozilla\/5\.0 /, "").slice(0, 90);
  $("row").value =
    `| ${new Date().toISOString().slice(0, 16)} | ${browser} | ${r.provider || "?"} | ${cfg.role} | ${r.step} | ` +
    `${r.ok ? "ok" : r.error} | UP${+!!f.UP} UV${+!!f.UV} BE${+!!f.BE} BS${+!!f.BS} | ${r.origin || ""} | ` +
    `${r.verified || ""} | asked? |`;
}

async function timed(step, f) {
  const t = performance.now();
  try {
    show({ step, ok: true, ...(await f()), ms: Math.round(performance.now() - t) });
  } catch (e) {
    show({ step, ok: false, error: `${e.name}: ${e.message}`, ms: Math.round(performance.now() - t) });
  }
}

async function register(rpId) {
  const cred = await navigator.credentials.create({
    publicKey: {
      rp: { id: rpId, name: "S46 test" },
      user: { id: crypto.getRandomValues(new Uint8Array(16)), name: `s46-${cfg.role}-${Date.now()}`, displayName: "S46 test" },
      challenge: crypto.getRandomValues(new Uint8Array(32)),
      pubKeyCredParams: [-7, -8, -257].map((alg) => ({ type: "public-key", alg })),
      authenticatorSelection: { residentKey: "required", userVerification: "required" },
      attestation: "none",
      timeout: 120000,
    },
  });
  const att = cbor(new Uint8Array(cred.response.attestationObject)).value;
  const ad = att.get("authData");
  const flags = flagsOf(ad[32]);
  const aaguid = hex(ad.slice(37, 53));
  const idLen = (ad[53] << 8) | ad[54];
  const credId = ad.slice(55, 55 + idLen);
  const coseKey = cbor(ad.slice(55 + idLen)).value;
  const k = await fromCose(coseKey);
  const rpOk = hex(ad.slice(0, 32)) === hex(await sha256(rpId));
  const cd = JSON.parse(new TextDecoder().decode(cred.response.clientDataJSON));
  await fetch("/log/append", {
    method: "POST",
    body: JSON.stringify({ event: "added", credential: b64u(credId), rp: rpId, ...k, aaguid }),
  });
  return {
    rpId, credential: b64u(credId), aaguid, provider: PROVIDERS[aaguid] || `unknown AAGUID ${aaguid}`,
    alg: k.alg, flags, rpIdHashOk: rpOk, origin: cd.origin, fmt: att.get("fmt"),
    transports: cred.response.getTransports?.(),
  };
}

async function sign(rpId) {
  const body = `arugula s46 test approval v1\nhost ${location.host}\nrp ${rpId}\nat ${Date.now()}\nnonce ${b64u(crypto.getRandomValues(new Uint8Array(8)))}\n`;
  const challenge = await sha256(body);
  const cred = await navigator.credentials.get({
    publicKey: { challenge, rpId, userVerification: "required", timeout: 120000 },
  });
  const r = cred.response;
  const ad = new Uint8Array(r.authenticatorData);
  const cd = JSON.parse(new TextDecoder().decode(r.clientDataJSON));
  const assertion = {
    id: b64u(cred.rawId),
    clientDataJSON: b64u(r.clientDataJSON),
    authenticatorData: b64u(r.authenticatorData),
    signature: b64u(r.signature),
  };
  const log = await (await fetch("/log")).json();
  const mine = log.find((e) => e.credential === assertion.id);
  const v = await verify({ log, account: cfg.account, assertion, challenge: b64u(challenge), at: Date.now(), origins: cfg.origins });
  return {
    rpId, credential: assertion.id, provider: mine ? PROVIDERS[mine.aaguid] || `unknown AAGUID ${mine.aaguid}` : "not in the log",
    flags: flagsOf(ad[32]), signCount: new DataView(ad.buffer).getUint32(33), origin: cd.origin,
    crossOrigin: cd.crossOrigin, topOrigin: cd.topOrigin, verified: v.ok ? "verified" : `refused: ${v.why}`,
    clientDataJSON: cd,
  };
}

$("reg-rp").onclick = () => timed("register rp", () => register(cfg.rp));
$("get-rp").onclick = () => timed(`sign rp on ${cfg.role}`, () => sign(cfg.rp));
$("reg-parent").onclick = () => timed("register parent", () => register(cfg.parent));
$("get-parent").onclick = () => timed(`sign parent on ${cfg.role}`, () => sign(cfg.parent));
$("copy").onclick = () => navigator.clipboard.writeText($("row").value);
window.__s46ready = true;
