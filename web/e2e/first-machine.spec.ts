// Which machine the app opens on (Arugula #1425): the one you were on last
// if control says it's online, else the first online one by name, and never
// one that's offline unless it's all there is or a link asked for it.
// Control's word on each machine is what the page has when its list comes,
// so the choice is made then, with no "Connecting to…" wait on a machine
// that's down.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";

type Listed = { name: string; online: boolean };

/** The page with `last` remembered as the machine shown (and `search` as
 * its address), given control's list: the machine it opens on. */
async function opens(page: Page, machines: Listed[], opts: { last?: string; search?: string } = {}): Promise<string> {
  const html = await fixturePage("./first-machine-page.tsx");
  await page.route("**/first-machine*", (r) => r.fulfill({ contentType: "text/html", body: html }));
  await page.addInitScript((last) => {
    if (last) localStorage.setItem("arugula.control.host", JSON.stringify(last));
  }, opts.last);
  await page.goto(`/first-machine${opts.search ?? ""}`);
  await page.waitForFunction(() => !!(window as unknown as { __listed?: unknown }).__listed);
  return page.evaluate(
    (machines) =>
      (window as unknown as { __listed(h: unknown[]): string }).__listed(
        machines.map((m) => ({ name: m.name, id: `id-${m.name}`, urls: [], transport: "control", added_ms: 0, last_seen_ms: null, status: m.online ? "online" : "offline" })),
      ),
    machines,
  );
}

const M1 = { name: "aaron-m1", online: false };
const DUST = { name: "dust", online: true };

test("the alphabetically first machine is offline: opens on the online one", async ({ page }) => {
  expect(await opens(page, [M1, DUST])).toBe("dust");
});

test("the machine you were last on wins when it's online", async ({ page }) => {
  expect(await opens(page, [{ name: "aaron-air", online: true }, M1, DUST], { last: "dust" })).toBe("dust");
});

test("the last machine is offline: the first online one, not the first the list gives", async ({ page }) => {
  expect(await opens(page, [M1, { name: "zed", online: true }, DUST], { last: "aaron-m1" })).toBe("dust");
});

test("a link to an offline machine still opens it", async ({ page }) => {
  expect(await opens(page, [M1, DUST], { search: "?machine=aaron-m1" })).toBe("aaron-m1");
});

test("every machine offline: as before, the first the list gives", async ({ page }) => {
  expect(await opens(page, [M1, { name: "dust", online: false }])).toBe("aaron-m1");
});
