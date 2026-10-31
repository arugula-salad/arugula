// The item block (#407) on its fixture page, in Chrome and in WebKit: each
// state draws its parts and, as much, leaves out the ones it has nothing to
// say for (no strip on a merged or closed item, no body when there's no
// description). The page is built from item-block-page.tsx with Vite and
// served by the test at /item-block.html, so no daemon or control is
// involved. Each state is photographed into test-results/ for the owner.
import { fileURLToPath } from "node:url";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { build } from "vite";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const ENTRY = fileURLToPath(new URL("./item-block-page.tsx", import.meta.url));

let html: Promise<string> | null = null;
/** The fixture page's HTML, script and style inline, built once per worker. */
function fixturePage(): Promise<string> {
  html ??= (async () => {
    const out = (await build({
      root: ROOT,
      configFile: false,
      oxc: { jsx: { runtime: "automatic", importSource: "preact" } },
      logLevel: "silent",
      build: { write: false, target: "es2022", lib: { entry: ENTRY, formats: ["es"], fileName: "page" }, cssCodeSplit: false },
    })) as { output: { type: string; fileName: string; code?: string; source?: string | Uint8Array }[] } | { output: unknown[] }[];
    const files = (Array.isArray(out) ? out.flatMap((o) => o.output) : out.output) as { type: string; fileName: string; code?: string; source?: string | Uint8Array }[];
    const js = files.find((f) => f.type === "chunk")?.code ?? "";
    const css = files.filter((f) => f.fileName.endsWith(".css")).map((f) => String(f.source)).join("\n");
    return `<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head><body><div id="root"></div><script type="module">${js.replaceAll("</script", "<\\/script")}</script></body></html>`;
  })();
  return html;
}

async function open(page: Page) {
  const body = await fixturePage();
  await page.route("**/item-block.html", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto("/item-block.html");
  await expect(page.locator("[data-item-block]")).toHaveCount(7);
}

/** A custom property's colour as the browser computes it. */
const cssColor = (page: Page, v: string) =>
  page.evaluate((v) => {
    const s = document.createElement("i");
    s.style.color = `var(${v})`;
    document.body.append(s);
    const c = getComputedStyle(s).color;
    s.remove();
    return c;
  }, v);

const SHOTS = "test-results";

export function itemBlockSpecs(tag: string) {
  const block = (page: Page, name: string): Locator => page.locator(`[data-fixture="${name}"] [data-item-block]`);
  const shot = (page: Page, name: string) => page.locator(`[data-fixture="${name}"]`).screenshot({ path: `${SHOTS}/item-block-${tag}-${name}.png` });

  test(`${tag}: an open PR that needs you draws all six parts`, async ({ page }) => {
    await open(page);
    const b = block(page, "pr-needs-you");
    await expect(b.locator("[data-item-head]")).toContainText("#1871");
    await expect(b.locator("[data-item-head]")).toContainText("Land: one rail everywhere");
    await expect(b.locator("[data-item-pill]")).toHaveText("Open");
    await expect(b.locator("[data-item-source]")).toHaveAttribute("data-live", "following");
    await expect(b.locator("[data-item-refresh]")).toHaveAttribute("title", "Read 2m ago");
    await expect(b.locator("[data-item-open]")).toHaveAttribute("href", "https://example.test/1");
    await expect(b.locator("[data-item-next='danger']")).toContainText("e2e (10) failed");
    await expect(b.locator("[data-item-next]")).toHaveCount(1);
    await expect(b.locator("[data-item-next] button")).toHaveText(["Rerun", "Open run ↗"]);
    // The strip's primary takes its tone (red here), with the page's background as its text.
    const [fill, danger, text, bg] = await Promise.all([
      b.locator("[data-item-next] .primary").evaluate((e) => getComputedStyle(e).backgroundColor),
      cssColor(page, "--danger"),
      b.locator("[data-item-next] .primary").evaluate((e) => getComputedStyle(e).color),
      cssColor(page, "--bg"),
    ]);
    expect(fill).toBe(danger);
    expect(text).toBe(bg);
    const facts = b.locator("[data-item-facts]");
    await expect(facts.locator("[data-item-fact='branch']")).toHaveText("land/rail-and-ux → main");
    await expect(facts.locator("[data-item-fact='checks']")).toHaveText(/✓ 29\s*● 1\s*✕ 1/);
    await expect(facts.locator("[data-item-avatar='reviewer'][data-name='jake']")).toHaveAttribute("title", "jake approved");
    await expect(facts.locator("[data-item-avatar='reviewer'][data-name='alec']")).toHaveAttribute("data-ring", "asked");
    await expect(facts.locator("[data-item-avatar='reviewer'][data-name='jake']")).not.toHaveAttribute("data-ring", /.*/);
    await expect(facts.locator("[data-item-fact='mergeable']")).toHaveAttribute("data-mergeable", "clean");
    await expect(facts.locator("[data-item-fact='comments']")).toHaveText(/3/);
    await expect(b.locator("[data-item-body]")).toBeVisible();
    await expect(b.locator("[data-item-tabs]")).toBeVisible();
    await expect(b.locator("[data-item-foot] button")).toHaveText(["Comment", "Diff", "Checkout"]);
    await shot(page, "pr-needs-you");
  });

  test(`${tag}: a ready PR gets a green strip with Merge, a dimmed older review and a hollow dot`, async ({ page }) => {
    await open(page);
    const b = block(page, "pr-ready");
    await expect(b.locator("[data-item-next='ok']")).toContainText("Ready to merge");
    await expect(b.locator("[data-item-next] button")).toHaveText("Merge");
    await expect(b.locator("[data-item-avatar='reviewer'][data-name='jake']")).toHaveAttribute("data-dim", "");
    await expect(b.locator("[data-item-avatar='reviewer'][data-name='jake']")).toHaveAttribute("title", /older commit/);
    await expect(b.locator("[data-item-source]")).toHaveAttribute("data-live", "paused");
    // Short text isn't folded, so there's nothing to unfold.
    await expect(b.locator("[data-item-more]")).toHaveCount(0);
    await shot(page, "pr-ready");
  });

  test(`${tag}: a merged PR has a purple pill and no strip`, async ({ page }) => {
    await open(page);
    const b = block(page, "pr-merged");
    await expect(b.locator("[data-item-pill]")).toHaveText("Merged");
    await expect(b).toHaveAttribute("data-item-state", "merged");
    await expect(b.locator("[data-item-next]")).toHaveCount(0);
    await expect(b.locator("[data-item-fact='text']")).toHaveText("merged 1cc9e39f · 2h");
    // The pill is the merged purple, not green or red.
    const [pill, purple] = await Promise.all([
      b.locator("[data-item-pill]").evaluate((e) => getComputedStyle(e).borderTopColor),
      page.evaluate(() => {
        const s = document.createElement("i");
        s.style.color = "var(--merged)";
        document.body.append(s);
        const c = getComputedStyle(s).color;
        s.remove();
        return c;
      }),
    ]);
    expect(pill).toBe(purple);
    await shot(page, "pr-merged");
  });

  test(`${tag}: closed and draft PRs`, async ({ page }) => {
    await open(page);
    const closed = block(page, "pr-closed");
    await expect(closed.locator("[data-item-pill]")).toHaveText("Closed");
    await expect(closed.locator("[data-item-next]")).toHaveCount(0);
    await expect(closed.locator("[data-item-fact='mergeable']")).toHaveAttribute("data-mergeable", "conflicts");
    // No ↻, ↗, tabs or footer were given, so none are drawn.
    await expect(closed.locator("[data-item-refresh], [data-item-open], [data-item-tabs], [data-item-foot]")).toHaveCount(0);
    await expect(closed.locator("[data-item-source]")).not.toHaveAttribute("data-live", /.*/);
    await shot(page, "pr-closed");
    const draft = block(page, "pr-draft");
    await expect(draft.locator("[data-item-pill]")).toHaveText("Draft");
    await expect(draft.locator("[data-item-next]")).toHaveCount(0);
    await expect(draft.locator("[data-item-fact='mergeable']")).toHaveAttribute("data-mergeable", "checking");
    await shot(page, "pr-draft");
  });

  test(`${tag}: an issue with an agent's question gets a yellow strip`, async ({ page }) => {
    await open(page);
    const b = block(page, "issue-question");
    await expect(b).toHaveAttribute("data-item-block", "issue");
    await expect(b.locator("[data-item-next='warn']")).toContainText("which pill colour");
    await expect(b.locator("[data-item-fact='labels']")).toContainText("web");
    await expect(b.locator("[data-item-avatar='reviewer']")).toHaveCount(0);
    await expect(b.locator("[data-item-fact='assignees']")).toBeVisible();
    await expect(b.locator("[data-item-tab='agents']")).toHaveAttribute("data-tone", "warn");
    await shot(page, "issue-question");
  });

  test(`${tag}: an issue with no description draws no body`, async ({ page }) => {
    await open(page);
    const b = block(page, "issue-empty");
    await expect(b.locator("[data-item-head]")).toContainText("#408");
    await expect(b.locator("[data-item-body]")).toHaveCount(0);
    await expect(b.getByText(/no description/i)).toHaveCount(0);
    await expect(b.locator("[data-item-next], [data-item-tabs]")).toHaveCount(0);
    await shot(page, "issue-empty");
  });

  test(`${tag}: tabs switch and More unfolds the body`, async ({ page }) => {
    await open(page);
    const b = block(page, "pr-needs-you");
    // The first tab is shown, and a failing one is red.
    await expect(b.locator("[data-item-panel]")).toHaveAttribute("data-item-panel", "checks");
    await expect(b.locator("[data-item-tab='checks']")).toHaveAttribute("data-tone", "danger");
    await expect(b.locator("[data-item-tab='checks']")).toContainText("31");
    await b.locator("[data-item-tab='reviews']").click();
    await expect(b.locator("[data-item-panel]")).toHaveAttribute("data-item-panel", "reviews");
    await expect(b.locator("[data-item-panel]")).toHaveText("jake approved");
    await expect(b.locator("[data-item-tab='reviews']")).toHaveAttribute("aria-selected", "true");
    await expect(b.locator("[data-item-tab='checks']")).toHaveAttribute("aria-selected", "false");
    // A long body is folded, and More shows all of it.
    const text = b.locator(".item-body-text");
    const folded = (await text.boundingBox())!.height;
    await expect(b.locator("[data-item-more]")).toHaveText("More");
    await b.locator("[data-item-more]").click();
    await expect(b.locator("[data-item-more]")).toHaveText("Less");
    expect((await text.boundingBox())!.height).toBeGreaterThan(folded + 10);
    await b.locator("[data-item-more]").click();
    await expect(b.locator("[data-item-more]")).toHaveText("More");
  });
}


