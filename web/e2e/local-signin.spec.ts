// A browser on the daemon's machine signs in as a person's does: no test
// header (the config's contexts carry one), only the sign-in link `arugula
// web` and the desktop app open (crates/daemon/src/localauth.rs). The link's
// page keeps the page key in this origin's storage, which is this port's
// alone, and the page sends it from there (shell/local-key.ts); nothing that
// grants access is a cookie, which a browser would send to every port.

import { expect, test, type Browser, type Page } from "@playwright/test";
import { localToken, pageKey, signIn } from "./local-token";

/** A browser with nothing: no header, no cookie, no storage. */
const bare = (browser: Browser) => browser.newContext({ extraHTTPHeaders: {}, storageState: { cookies: [], origins: [] } });

const connected = (page: Page) => page.evaluate(() => !!window.__arugula?.client?.state);

test("the sign-in link gives the page its key, and the page connects with it", async ({ browser, baseURL }) => {
  const ctx = await bare(browser);
  const page = await ctx.newPage();
  await page.goto(signIn(baseURL!, "/#x=1"));
  await expect(page).toHaveURL(`${baseURL}/#x=1`);
  await expect.poll(() => connected(page)).toBe(true);
  expect(await page.evaluate(() => localStorage.getItem("arugula.key"))).toBe(pageKey);
  // No cookie: nothing a server on another port would be sent.
  expect(await ctx.cookies()).toEqual([]);
  // A reload stays signed in, from storage.
  await page.reload();
  await expect.poll(() => connected(page)).toBe(true);
  await ctx.close();
});

test("without the key, the page goes to say how to sign in", async ({ browser, baseURL }) => {
  const ctx = await bare(browser);
  const page = await ctx.newPage();
  await page.goto(`${baseURL}/`);
  await expect(page).toHaveURL(new RegExp(`${baseURL}/auth\\?next=`));
  await expect(page.getByText("arugula web")).toBeVisible();
  await ctx.close();
});

test("a browser signed in by an older link's cookie is signed in again from it, once", async ({ browser, baseURL }) => {
  const ctx = await bare(browser);
  const host = new URL(baseURL!).hostname;
  await ctx.addCookies([{ name: `arugula_${new URL(baseURL!).port}`, value: localToken, domain: host, path: "/", httpOnly: true, sameSite: "Lax" }]);
  const page = await ctx.newPage();
  await page.goto(`${baseURL}/#y=2`);
  // The cookie alone isn't let in; the page finds its way to the key.
  await expect(page).toHaveURL(`${baseURL}/#y=2`);
  await expect.poll(() => connected(page)).toBe(true);
  expect(await page.evaluate(() => localStorage.getItem("arugula.key"))).toBe(pageKey);
  expect(await ctx.cookies(), "the old cookie is gone").toEqual([]);
  await ctx.close();
});

test("a cookie with the token gets nothing from the API", async ({ playwright, baseURL }) => {
  const api = await playwright.request.newContext({ extraHTTPHeaders: {} });
  const port = new URL(baseURL!).port;
  const res = await api.get(`${baseURL}/api/panes`, { headers: { cookie: `arugula_${port}=${localToken}`, "sec-fetch-site": "same-origin" } });
  expect(res.status()).toBe(401);
  expect(res.headers()["x-arugula-sign-in"]).toBe("/auth");
  expect((await api.get(`${baseURL}/api/panes`, { headers: { "x-arugula-key": pageKey } })).status()).toBe(200);
  await api.dispose();
});
