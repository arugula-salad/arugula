// A team Doc's pictures (Arugula #1409): the platform keeps them and serves
// each at `…/docs/<id>/assets/<name>`, and wherever a project Doc is drawn
// an `assets/` image is that picture, loaded, not its alt text: the Doc's
// page and the Doc's embed page for the app. On
// #971's fixture page (move-to-project-page.tsx) and the embed page; the
// platform's answers are this spec's.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { png } from "./doc-steps";

const SITE = "prj_aaaa0002";
const DOC = "pdc_aaaa0001";
const ALT = "The Overview zoomed to the board";

/** The platform's answers for one team Doc with a picture; the picture's reads are counted. */
async function answers(page: Page): Promise<{ reads: number }> {
  const seen = { reads: 0 };
  const now = Date.now();
  const dot = png(4, 3);
  const doc = { id: DOC, project: SITE, title: "Framing check", author: "acct_a", agent: null, sections: 1, written: 1, rev: 1, created_at: now, updated_at: now };
  const view = {
    doc,
    sections: [{ id: "s1", heading: "Overview", intent: "", body: `How it frames:\n\n![${ALT}](assets/overview.png)`, rev: 1 }],
    assets: [{ name: "overview.png", mime: "image/png", size: dot.length, rev: 1 }],
    writer: false,
    deleter: false,
    link: null,
  };
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/control\.json$/, (r) => json(r, { proto: null, platform: "https://app.test" }));
  await page.route(/\/api\/projects$/, (r) =>
    json(r, {
      me: { account: "acct_a", login: "sam", name: "Sam" },
      projects: [{ id: SITE, name: "Site", owner: { kind: "team", id: "tm_1", name: "Acme" }, team: { id: "tm_1", name: "Acme", role: "viewer", members: [{ account: "acct_a", name: "Sam" }] } }],
      hosted: false,
    }),
  );
  await page.route(/\/api\/projects\/[^/]+\/docs$/, (r) => json(r, { project: SITE, docs: [doc], writer: false }));
  await page.route(new RegExp(`/api/projects/[^/]+/docs/${DOC}$`), (r) => json(r, view));
  await page.route(new RegExp(`/api/projects/[^/]+/docs/${DOC}/assets/overview\\.png$`), (r) => {
    seen.reads += 1;
    return r.fulfill({ status: 200, contentType: "image/png", body: dot });
  });
  return seen;
}

/** The picture is drawn, decoded, and its alt text isn't standing in. */
async function expectPicture(page: Page) {
  const img = page.locator(`img.doc-image[alt="${ALT}"]`);
  await expect(img).toHaveAttribute("src", /^data:image\/png;base64,/);
  await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth)).toBe(4);
  await expect(page.locator(".doc-image-missing, .doc-image-wait")).toHaveCount(0);
}

test.use({ viewport: { width: 1280, height: 900 } });

test("the Doc's page shows a picture the platform keeps, not its alt text", async ({ page }) => {
  const seen = await answers(page);
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(`/p/${SITE}/docs/${DOC}`);
  await expectPicture(page);
  expect(seen.reads).toBe(1);
});

test("the Doc's embed page for the app shows it", async ({ page }) => {
  await answers(page);
  const body = await fixturePage("../src/shell/doc-embed.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(`/embed/p/${SITE}/docs/${DOC}`);
  await expect(page.locator("[data-embed] .doc-title")).toHaveText("Framing check");
  await expectPicture(page);
});
