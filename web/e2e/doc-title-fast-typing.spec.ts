// A Doc's title typed the moment its editor opens (Arugula #991): the draft
// is set as the editor opens, not after it's drawn, so no key typed at once
// is overwritten by the old title. Both ways in, ⋯'s Rename and a click on
// the title, type straight away, with no wait for the field's value, and
// the new title saves. On #971's fixture page (move-to-project-page.tsx):
// the platform's answers are this spec's.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const SITE = "prj_aaaa0002";

/** The fixture page at a Doc its reader may write; the titles it was sent are kept in the list returned. */
async function open(page: Page): Promise<string[]> {
  const sent: string[] = [];
  const now = Date.now();
  const doc = { id: "pdc_aaaa0001", project: SITE, title: "Launch plan", author: "acct_a", agent: null, sections: 1, written: 1, rev: 1, created_at: now, updated_at: now };
  const view = () => ({ doc, sections: [{ id: "s1", heading: "Why", intent: "", body: "What it's for.", rev: 1 }], assets: [], writer: true, deleter: true, link: null });
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) =>
    json(r, {
      me: { account: "acct_a", login: "sam", name: "Sam" },
      projects: [{ id: SITE, name: "Site", owner: { kind: "team", id: "tm_1", name: "Acme" }, team: { id: "tm_1", name: "Acme", role: "owner", members: [] } }],
      hosted: false,
    }),
  );
  await page.route(/\/api\/projects\/[^/]+\/docs\/pdc_aaaa0001$/, (r) => {
    if (r.request().method() === "PUT") {
      const title = (r.request().postDataJSON() as { title: string }).title;
      sent.push(title);
      doc.title = title;
      doc.rev += 1;
    }
    return json(r, view());
  });
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(`/p/${SITE}/docs/pdc_aaaa0001`);
  await expect(page.locator("[data-doc-title-edit]")).toHaveText("Launch plan");
  return sent;
}

test("a title typed right after ⋯'s Rename keeps every key, and saves", async ({ page }) => {
  const sent = await open(page);
  await page.locator("[data-doc-more]").first().click();
  await page.locator("[data-doc-rename]").click();
  // No wait for the field's value: the keys go to whatever has them now.
  await page.keyboard.type("Launch plan, v2");
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-doc-title-edit]")).toHaveText("Launch plan, v2");
  expect(sent).toEqual(["Launch plan, v2"]);
});

test("a title typed right after clicking it keeps every key, and saves", async ({ page }) => {
  const sent = await open(page);
  await page.locator("[data-doc-title-edit]").click();
  await page.keyboard.type("Pricing");
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-doc-title-edit]")).toHaveText("Pricing");
  expect(sent).toEqual(["Pricing"]);
});
