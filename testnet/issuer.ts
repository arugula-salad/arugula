// A product signing a person in through the hub's issuer on the testnet
// (M228), as studio or a hud box would: no SDK, only fetch and node:crypto.
// One JSON object on stdout, exit 0; or why on stderr, exit 1.
//
//   node --experimental-strip-types testnet/issuer.ts COMMAND --control URL [--via FROM=TO ...] ...
//
//   signin --login L --client ID --redirect URI --token-out FILE --jwks-out FILE
//               signed out, /oauth/authorize sends the browser to control's
//               page to sign in (#issuer-signin=, the same request); signed
//               in with (the fake) GitHub, it comes back with a code; the
//               code and its PKCE verifier get an ID token, checked against
//               the JWKS (signature by its kid, iss, aud, minutes long, no
//               project or role); userinfo says the same account. Writes the
//               token and the JWKS it was checked with:
//               {kid, sub, github_id, jwks: [kid...]}
//   verify --client ID --token FILE [--jwks FILE]
//               the token against the JWKS in FILE (a client's cached copy),
//               or fetched now: {kid, sub}; exit 1 when it doesn't verify
//   hud --login L --client ID --redirect URI --spki BASE64 [--assertion-out FILE]
//               a hud box's sign-in: /oauth/hud's assertion in the box page's
//               fragment, checked as hud's proxy/forge-signin.ts checks it,
//               with the SPKI a box is given: {sub, aud, kid}
//   link --login L --client ID --redirect URI --secret S --user U [--skip-unlink 1]
//               M229, a product linking its own user U: signs in as above,
//               then from the product's "server" (HTTP Basic, its link
//               secret) POST /oauth/links with the token's numeric github_id
//               (linked, then the same again: no change); with another
//               numeric id, refused (logins change hands); the person's
//               platform page lists it and they unlink it there (unless
//               --skip-unlink); the product's next GET says not_linked:
//               {platform_account, sub, created, again, mismatch, listed, after}

import { createHash, createPublicKey, randomBytes, verify, type KeyObject } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const argv = process.argv.slice(2);
const cmd = argv.shift();
const opts: Record<string, string> = {};
const via: [string, string][] = [];
for (let i = 0; i < argv.length; i += 2) {
  const [k, v] = [argv[i].replace(/^--/, ""), argv[i + 1]];
  if (k === "via") {
    const at = v.indexOf("=");
    via.push([v.slice(0, at), v.slice(at + 1)]);
  } else opts[k] = v;
}
const need = (k: string) => {
  if (!opts[k]) throw new Error(`--${k} is required`);
  return opts[k];
};
const control = need("control");
const reach = (u: string) => {
  for (const [from, to] of via) if (u.startsWith(from)) return to + u.slice(from.length);
  return u;
};
const b64url = (b: Buffer) => b.toString("base64url");
const fail = (why: string): never => {
  throw new Error(why);
};

type Jwk = { kty: string; crv: string; x: string; kid: string };

/** A browser: cookies by name, redirects followed by hand. */
class Browser {
  jar: Record<string, string> = {};
  async get(url: string): Promise<Response> {
    const res = await fetch(reach(url), {
      redirect: "manual",
      headers: { cookie: Object.entries(this.jar).map(([k, v]) => `${k}=${v}`).join("; ") },
    });
    for (const c of res.headers.getSetCookie()) {
      const [kv] = c.split(";");
      const at = kv.indexOf("=");
      this.jar[kv.slice(0, at)] = kv.slice(at + 1);
    }
    return res;
  }
}

/** Signed out: the hub sends `path` through control's page and back. Then
 * a GitHub sign-in that ends on `path`, followed until it leaves control
 * for `stop`: that address. */
async function signInThrough(b: Browser, path: string, login: string, stop: (u: string) => boolean): Promise<string> {
  const first = await b.get(control + path);
  const loc = first.headers.get("location") ?? "";
  const m = /^\/#issuer-signin=([A-Za-z0-9_-]+)$/.exec(loc);
  if (first.status !== 303 || !m) fail(`signed out, ${path} answered ${first.status} ${loc}`);
  if (Buffer.from(m![1], "base64url").toString() !== path) fail("#issuer-signin= isn't the request it came from");
  // What control's page does once signed in: GitHub, then back to it.
  let url = `${control}/auth/github?next=${encodeURIComponent(path)}`;
  for (let i = 0; i < 8; i++) {
    const u = new URL(url);
    if (u.pathname === "/login/oauth/authorize") u.searchParams.set("login", login);
    const res = await b.get(u.href);
    const next = res.headers.get("location");
    if (!next) fail(`signing in: ${res.status} at ${u.pathname}: ${(await res.text()).slice(0, 200)}`);
    url = new URL(next!, url).href;
    if (stop(url)) return url;
  }
  return fail("signing in went round too many times");
}

function verifyJwt(token: string, jwks: { keys: Jwk[] }, client: string): { kid: string; claims: Record<string, unknown> } {
  const [h, p, s] = token.split(".");
  const header = JSON.parse(Buffer.from(h, "base64url").toString());
  if (header.alg !== "EdDSA") fail(`alg ${header.alg}`);
  const jwk = jwks.keys.find((k) => k.kid === header.kid) ?? fail(`kid ${header.kid} isn't in the JWKS`);
  const key = createPublicKey({ key: { kty: jwk.kty, crv: jwk.crv, x: jwk.x }, format: "jwk" });
  if (!verify(null, Buffer.from(`${h}.${p}`), key, Buffer.from(s, "base64url"))) fail(`bad signature by ${header.kid}`);
  const claims = JSON.parse(Buffer.from(p, "base64url").toString());
  const now = Date.now() / 1000;
  if (claims.iss !== control) fail(`iss ${claims.iss}`);
  if (claims.aud !== client) fail(`aud ${claims.aud}`);
  if (!(claims.exp > now && claims.exp - claims.iat <= 600)) fail(`lifetime ${claims.iat}..${claims.exp}`);
  for (const k of Object.keys(claims)) if (/project|role|group|scope/i.test(k)) fail(`a ${k} claim`);
  return { kid: header.kid, claims };
}

async function jwksNow(): Promise<{ keys: Jwk[] }> {
  const r = await fetch(reach(`${control}/.well-known/jwks.json`));
  if (!r.ok) fail(`JWKS: ${r.status}`);
  return r.json();
}

/** Signed in through the hub as a product: the browser (with control's
 * session), the ID token, its claims and the access token. */
async function signedIn(login: string, client: string, redirect: string) {
  const b = new Browser();
  const verifier = b64url(randomBytes(32));
  const challenge = b64url(createHash("sha256").update(verifier).digest());
  const state = b64url(randomBytes(12));
  const nonce = b64url(randomBytes(12));
  const q = new URLSearchParams({
    response_type: "code",
    client_id: client,
    redirect_uri: redirect,
    scope: "openid",
    state,
    nonce,
    code_challenge: challenge,
    code_challenge_method: "S256",
  });
  const back = await signInThrough(b, `/oauth/authorize?${q}`, login, (u) => u.startsWith(redirect));
  const got = new URL(back).searchParams;
  if (got.get("state") !== state || !got.get("code")) fail(`back without a code: ${back}`);
  const res = await fetch(reach(`${control}/oauth/token`), {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "authorization_code", code: got.get("code")!, redirect_uri: redirect, client_id: client, code_verifier: verifier }),
  });
  const t = await res.json();
  if (!res.ok) fail(`token: ${res.status} ${JSON.stringify(t)}`);
  const jwks = await jwksNow();
  const { kid, claims } = verifyJwt(t.id_token, jwks, client);
  if (claims.nonce !== nonce) fail("nonce");
  if (typeof claims.github_id !== "number") fail("no numeric github_id");
  const me = await (await fetch(reach(`${control}/oauth/userinfo`), { headers: { authorization: `Bearer ${t.access_token}` } })).json();
  if (me.sub !== claims.sub || me.github_id !== claims.github_id) fail(`userinfo ${JSON.stringify(me)}`);
  return { b, t, jwks, kid, claims };
}

async function signin() {
  const { t, jwks, kid, claims } = await signedIn(need("login"), need("client"), need("redirect"));
  writeFileSync(need("token-out"), t.id_token);
  writeFileSync(need("jwks-out"), JSON.stringify(jwks));
  return { kid, sub: claims.sub, github_id: claims.github_id, jwks: jwks.keys.map((k) => k.kid) };
}

async function check() {
  const token = readFileSync(need("token"), "utf8").trim();
  const jwks = opts.jwks ? JSON.parse(readFileSync(opts.jwks, "utf8")) : await jwksNow();
  const { kid, claims } = verifyJwt(token, jwks, need("client"));
  return { kid, sub: claims.sub };
}

async function hud() {
  const [login, client, redirect] = [need("login"), need("client"), need("redirect")];
  // As hud reads HUD_FORGE_SIGNIN_KEY: base64 of the DER SPKI, Ed25519.
  const key: KeyObject = createPublicKey({ key: Buffer.from(need("spki"), "base64"), format: "der", type: "spki" });
  if (key.asymmetricKeyType !== "ed25519") fail(`a ${key.asymmetricKeyType} key`);
  const q = new URLSearchParams({ client_id: client, redirect_uri: redirect });
  const back = await signInThrough(new Browser(), `/oauth/hud?${q}`, login, (u) => u.startsWith(redirect));
  const m = /#hud-forge=(hudfs1\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)$/.exec(back) ?? fail(`no assertion: ${back}`);
  const parts = m[1].split(".");
  if (!verify(null, Buffer.from(`${parts[0]}.${parts[1]}`, "ascii"), key, Buffer.from(parts[2], "base64url"))) fail("not signed by the box's key");
  const p = JSON.parse(Buffer.from(parts[1], "base64url").toString("utf8"));
  const now = Date.now() / 1000;
  if (p.aud !== client) fail(`aud ${p.aud}`);
  if (p.sub !== `github:${login}`) fail(`sub ${p.sub}`);
  if (!(p.exp - p.iat <= 600 && p.exp > p.iat && p.iat <= now + 60 && p.exp > now)) fail("lifetime");
  if (typeof p.jti !== "string" || p.jti.length < 8 || p.jti.length > 200) fail("jti");
  if (opts["assertion-out"]) writeFileSync(opts["assertion-out"], m[1]);
  return { sub: p.sub, aud: p.aud, kid: p.kid };
}

async function link() {
  const [client, secret, user] = [need("client"), need("secret"), need("user")];
  const { b, t, claims } = await signedIn(need("login"), client, need("redirect"));
  const basic = `Basic ${Buffer.from(`${client}:${secret}`).toString("base64")}`;
  const product = async (method: string, path: string, body?: unknown) => {
    const r = await fetch(reach(control + path), {
      method,
      headers: { authorization: basic, ...(body ? { "content-type": "application/json" } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    return { status: r.status, v: (await r.json().catch(() => ({}))) as Record<string, unknown> };
  };
  const gh = claims.github_id as number;
  const send = (github_id: number) => product("POST", "/oauth/links", { user, github_id, assertion: t.id_token, name: `${user} (testnet)` });
  const first = await send(gh);
  if (first.status !== 200) fail(`link: ${first.status} ${JSON.stringify(first.v)}`);
  if (first.v.platform_account !== claims.sub) fail(`linked to ${first.v.platform_account}, not ${claims.sub}`);
  const again = await send(gh);
  if (again.status !== 200 || again.v.created !== false) fail(`the same link again: ${again.status} ${JSON.stringify(again.v)}`);
  // The same person, but the product says another numeric id: a login that
  // changed hands. Never a link.
  const other = await product("POST", "/oauth/links", { user: `${user}-other`, github_id: gh + 1, assertion: t.id_token });
  if (other.status !== 403 || other.v.error !== "github_id_mismatch") fail(`another numeric id: ${other.status} ${JSON.stringify(other.v)}`);
  // The person's platform page, on control's site with their session.
  const cookie = Object.entries(b.jar).map(([k, v]) => `${k}=${v}`).join("; ");
  const page = async (method: string, path: string) => {
    const r = await fetch(reach(control + path), { method, headers: { cookie, origin: control } });
    return { status: r.status, v: (await r.json().catch(() => ({}))) as Record<string, unknown> };
  };
  const listed = await page("GET", "/api/linked-products");
  const links = (listed.v.links ?? []) as { client: string; user: string }[];
  if (listed.status !== 200 || !links.some((l) => l.client === client && l.user === user)) fail(`the platform page lists ${listed.status} ${JSON.stringify(listed.v)}`);
  if (!opts["skip-unlink"]) {
    const u = await page("POST", `/api/linked-products/${encodeURIComponent(client)}/${encodeURIComponent(user)}/unlink`);
    if (u.status !== 200) fail(`unlink on the platform page: ${u.status} ${JSON.stringify(u.v)}`);
  }
  // The product sees it on its next call.
  const after = await product("GET", `/oauth/links/${encodeURIComponent(user)}`);
  if (after.status !== 404 || after.v.error !== "not_linked") fail(`the product still sees a link after the unlink: ${after.status} ${JSON.stringify(after.v)}`);
  return {
    platform_account: first.v.platform_account,
    sub: claims.sub,
    github_id: gh,
    created: first.v.created,
    again: again.v.created,
    mismatch: other.v.error,
    listed: links.length,
    after: after.v.error,
  };
}

const run = { signin, verify: check, hud, link }[cmd ?? ""];
if (!run) {
  console.error("usage: issuer.ts signin|verify|hud|link --control URL ...");
  process.exit(2);
}
run()
  .then((r) => console.log(JSON.stringify(r)))
  .catch((e) => {
    console.error(`issuer.ts ${cmd}: ${e instanceof Error ? e.message : e}`);
    process.exit(1);
  });
