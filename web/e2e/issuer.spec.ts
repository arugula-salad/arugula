// The hub as an issuer (M228), through control's real page: a product sends
// a signed-out browser to /oauth/authorize; control's page signs them in
// with GitHub (`#issuer-signin=`) and the browser comes back to the product
// with a code, which the product swaps for an ID token that verifies against
// the JWKS. Signed in now, the next sign-in goes straight through.

import { execFileSync } from "node:child_process";
import { createHash, createPublicKey, randomBytes, verify } from "node:crypto";
import { createServer, type Server } from "node:http";
import { expect, test } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("issuer");
const CLIENT = "e2e-product";
// The product's callback: a page that only says what it is.
let product: Server;
let BACK = "";

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  product?.close();
  t.stop();
});

const b64url = (b: Buffer) => b.toString("base64url");

test("a signed-out person signs in on control's page and comes back to the product", async ({ browser }) => {
  test.setTimeout(120_000);
  product = createServer((_req, res) => res.writeHead(200, { "content-type": "text/plain" }).end("the product"));
  await new Promise<void>((r) => product.listen(0, "127.0.0.1", r));
  BACK = `http://127.0.0.1:${(product.address() as { port: number }).port}/cb`;
  await t.start(["--issuer-key", randomBytes(32).toString("hex")]);
  execFileSync("../target/debug/arugula-control", ["issuer", "add-client", "--db", t.db, "--id", CLIENT, "--name", "E2E", "--redirect", BACK]);

  const ctx = await browser.newContext({ baseURL: t.base });
  await ctx.addCookies([{ name: "as", value: "ida", url: t.github }]);
  const page = await ctx.newPage();

  const authorize = () => {
    const verifier = b64url(randomBytes(32));
    const q = new URLSearchParams({
      response_type: "code",
      client_id: CLIENT,
      redirect_uri: BACK,
      scope: "openid",
      state: "st-e2e",
      code_challenge: b64url(createHash("sha256").update(verifier).digest()),
      code_challenge_method: "S256",
    });
    return { verifier, path: `/oauth/authorize?${q}` };
  };

  // Signed out: control's page, to sign in, with the request in its
  // fragment; signing in comes back to it, and on to the product.
  const first = authorize();
  await page.goto(first.path);
  await expect(page).toHaveURL(new RegExp(`^${t.base}/#issuer-signin=`));
  await page.locator("[data-signin=github]").click();
  await page.waitForURL(`${BACK}?**`, { timeout: 30_000 });
  const back = new URL(page.url());
  expect(back.searchParams.get("state")).toBe("st-e2e");

  // The product swaps the code and checks the ID token against the JWKS.
  const res = await fetch(`${t.base}/oauth/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "authorization_code", code: back.searchParams.get("code")!, redirect_uri: BACK, client_id: CLIENT, code_verifier: first.verifier }),
  });
  expect(res.status).toBe(200);
  const { id_token } = (await res.json()) as { id_token: string };
  const jwks = (await (await fetch(`${t.base}/.well-known/jwks.json`)).json()) as { keys: { kid: string; kty: string; crv: string; x: string }[] };
  const [h, p, s] = id_token.split(".");
  const kid = JSON.parse(Buffer.from(h, "base64url").toString()).kid;
  const jwk = jwks.keys.find((k) => k.kid === kid)!;
  const key = createPublicKey({ key: { kty: jwk.kty, crv: jwk.crv, x: jwk.x }, format: "jwk" });
  expect(verify(null, Buffer.from(`${h}.${p}`), key, Buffer.from(s, "base64url"))).toBe(true);
  const claims = JSON.parse(Buffer.from(p, "base64url").toString());
  expect(claims.aud).toBe(CLIENT);
  expect(typeof claims.github_id).toBe("number");

  // Signed in now: straight through, no page of control's drawn.
  const second = authorize();
  await page.goto(`${t.base}${second.path}`);
  await page.waitForURL(`${BACK}?**`, { timeout: 15_000 });
  expect(new URL(page.url()).searchParams.get("code")).toBeTruthy();
});
