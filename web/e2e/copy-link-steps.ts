// The Copy link button (#411) on a board's page and a project Doc's page: one
// click copies the page's team link and says "Copied ✓" in place. Alice owns
// team Acme, with chat's and boards' Labs flags on; her board and Doc are
// made through the platform's API. A local control (team-fixture.ts,
// `--platform-host app.localhost`), as boards-platform.spec.ts. Chrome reads
// the link back from the clipboard; WebKit can't read it in a test, so
// there the button's `data-link` (what it copies) stands in, and it says
// "Copied ✓" only once the write succeeded.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

export function copyLinkSpecs(name: string, clipboard: boolean, shots = false) {
  const t = new TeamControl(name);
  let alice: Page;
  let app = "";
  let project = "";

  test.describe.configure({ mode: "serial" });
  test.afterAll(async ({ browser }) => {
    await closeContexts({ browser });
    t.stop();
  });

  async function api(method: string, path: string, body: unknown) {
    return alice.evaluate(
      async ([m, u, b]) => {
        const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify(b) });
        return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
      },
      [method, path, body] as const,
    );
  }

  async function open(path: string, ready: string) {
    await alice.goto(`${app}${path}`);
    await expect(alice.locator(ready)).toBeVisible({ timeout: 15_000 });
  }

  /** One click on Copy link: "Copied ✓" in place, and the link it copied. */
  async function copyLink(expected: string, shot: string) {
    const button = alice.locator("[data-copy-link]");
    const main = button.locator("[data-copy-link-main]");
    await expect(button).toHaveCount(1);
    await expect(button).toHaveAttribute("data-link", expected);
    await expect(main).toHaveAttribute("title", "Copy link");
    await expect(main).toContainText("Copy link");
    if (shots) await alice.screenshot({ path: `test-results/copy-link-${shot}.png` });
    await main.click();
    await expect(main).toContainText("Copied ✓");
    if (shots) await alice.screenshot({ path: `test-results/copy-link-${shot}-copied.png` });
    if (clipboard) expect(await alice.evaluate(() => navigator.clipboard.readText())).toBe(expected);
    // And back to "Copy link" a moment later.
    await expect(main).toContainText("Copy link", { timeout: 5_000 });
  }

  test("Copy link on a board's page copies its team link", async ({ browser }) => {
    test.setTimeout(180_000);
    await t.start(["--platform-host", "app.localhost"]);
    app = t.base.replace("//127.0.0.1", "//app.localhost");
    alice = await t.person(browser, "alice");
    if (clipboard) await alice.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: app });
    await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
    const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
    for (const flag of ["chat", "boards"]) await alice.evaluate(([f, tm]) => window.__arugula.control!.setLabsFlag(f, true, tm), [flag, team] as const);

    await alice.goto(`${app}/`);
    await expect(alice.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
    await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
    project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

    const made = await api("POST", `/api/projects/${project}/boards`, { title: "Launch" });
    expect(made.status).toBe(200);
    const board = made.body!.board.id as string;
    await open(`/p/${project}/boards/${board}`, "[data-board-page] [data-board-title]");
    await copyLink(`${app}/p/${project}/boards/${board}`, "board");
  });

  test("Copy link on a project Doc's page copies its team link", async () => {
    test.setTimeout(90_000);
    const made = await api("POST", `/api/projects/${project}/docs`, { title: "Release plan", sections: [] });
    expect(made.status).toBe(200);
    const doc = made.body!.doc.id as string;
    await open(`/p/${project}/docs/${doc}`, "[data-doc-page] .doc-title");
    // The page's address names the Doc by id and a slug; the link is that address.
    await copyLink(alice.url(), "doc");
    await expect(alice.locator("[data-copy-link]")).toHaveAttribute("data-link", new RegExp(`^${app}/p/${project}[^/]*/docs/${doc}`));
  });
}
