// M108 (#1023) and M113 (#1028): the workspace on a computer. The rail, the
// session sidebar (switching, search), the routes and their way back, the
// header's connection chip, and the states and accessibility M113 made
// deliberate.

import { expect, test, type Page } from "@playwright/test";
import { active, panes, reset, tab, tabsInSession } from "./helpers";

async function newWorkspace(page: Page) {
  await reset(page);
  await expect(page.locator(".ws-rail")).toBeVisible();
}

/** A new session named `name`, shown once it's there. */
async function session(page: Page, name: string): Promise<number> {
  await page.evaluate((name) => window.__arugula.client.intent({ op: "new_session", name, from_pane: null }), name);
  let id: number | undefined;
  await expect
    .poll(async () => (id = await page.evaluate((n) => window.__arugula.client.state!.sessions.find((s) => s.name === n)?.id, name)))
    .not.toBeUndefined();
  return id!;
}

const shownSession = (page: Page) => page.evaluate(() => window.__arugula.client.session);
const row = (page: Page, name: string) => page.locator(".ws-session", { has: page.locator(".ws-session-name", { hasText: new RegExp(`^${name}$`) }) });

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("the sidebar switches sessions, and a session comes back at its last tab", async ({ page }) => {
    await newWorkspace(page);
    const first = await shownSession(page);
    // A second tab in the first session, shown.
    await page.locator(".ws-tabs .new-tab").click();
    await expect.poll(() => tabsInSession(page)).toHaveLength(2);
    const [, second] = await tabsInSession(page);
    await page.locator(`[data-tab-id="${second}"]`).click();
    await expect.poll(() => tab(page).then((t) => t.id)).toBe(second);

    const beta = await session(page, "beta");
    await row(page, "beta").locator(".ws-session-main").click();
    await expect.poll(() => shownSession(page)).toBe(beta);
    await expect(page.locator(".ws-crumb")).toContainText("beta");
    await expect(row(page, "beta")).toHaveAttribute("aria-selected", "true");

    // Back: the tab that was shown, not the session's first.
    const firstName = await page.evaluate((s) => window.__arugula.client.state!.sessions.find((x) => x.id === s)!.name, first);
    await row(page, firstName).locator(".ws-session-main").click();
    await expect.poll(() => shownSession(page)).toBe(first);
    await expect.poll(() => tab(page).then((t) => t.id)).toBe(second);

    // Rows say what they hold, from state.
    await expect(row(page, firstName).locator(".ws-session-counts")).toHaveAttribute("aria-label", "2 tabs, 2 panes");
    await expect(page.locator('[data-session-group="idle"], [data-session-group="working"]').first()).toBeVisible();
  });

  test("session attention and actions never overlap timestamps at compact widths", async ({ page }) => {
    await newWorkspace(page);
    await page.evaluate(() => {
      const c = window.__arugula.client;
      c.sleep(); // Freeze this layout fixture against live activity deltas.
      const p = c.state!.panes.find((p) => p.id === c.active())!;
      p.attention = "needs_input";
      p.activity = { ...p.activity!, last_ms: Date.now() };
      p.branch = "a-long-branch-name-that-needs-to-truncate";
      c.emit();
    });
    const r = page.locator('.ws-session[data-attention="needs_input"]');
    await expect(r.locator(".att")).toBeVisible();
    for (const width of [200, 248, 380]) {
      await page.locator(".ws-sidebar").evaluate((el, w) => { el.style.width = `${w}px`; }, width);
      for (const time of ["now", "59m ago", "123d ago"]) {
        await r.locator(".ws-session-ago").evaluate((el, t) => { el.textContent = t; }, time);
        const boxes = await Promise.all([".ws-session-ago", ".att", ".ws-session-more"].map((s) => r.locator(s).boundingBox()));
        const [ago, badge, more] = boxes.map((b) => b!);
        expect(ago.x + ago.width).toBeLessThanOrEqual(badge.x);
        expect(badge.x + badge.width).toBeLessThanOrEqual(more.x);
        const outer = (await r.boundingBox())!;
        expect(more.x + more.width).toBeLessThanOrEqual(outer.x + outer.width);
      }
    }
  });

  test("macOS reserves the native controls strip once, including with native tabs", async ({ page }) => {
    await newWorkspace(page);
    expect((await page.locator(".ws-head").boundingBox())!.y).toBe(0);
    await page.evaluate(() => { document.documentElement.dataset.desktop = "macos"; });
    const header = (await page.locator(".ws-head").boundingBox())!;
    const brand = (await page.locator(".ws-brand").boundingBox())!;
    expect(header.y).toBe(28);
    expect(brand.y).toBe(header.y);
    expect((await page.locator(".ws-wordmark").boundingBox())!.y).toBeGreaterThanOrEqual(28);
    await page.evaluate(() => {
      document.documentElement.dataset.nativeTabs = "";
      document.documentElement.style.setProperty("--native-tabs", "64px");
    });
    expect((await page.locator(".ws-head").boundingBox())!.y).toBe(64);
    expect((await page.locator(".ws-brand").boundingBox())!.y).toBe(64);
  });

  test("rail routes keep the way back: Panes returns to the same tab and pane", async ({ page }) => {
    await newWorkspace(page);
    const t = (await tab(page)).id;
    const pane = await active(page);
    const rail = page.locator(".ws-rail");

    await rail.locator("[data-open-swarm]").click();
    await expect.poll(() => page.evaluate(() => location.hash)).toBe("#swarm");
    await expect(rail.locator("[data-open-swarm]")).toHaveAttribute("aria-pressed", "true");
    await expect(rail.locator("[data-open-swarm]")).toHaveAttribute("aria-label", /all machines/);
    // The swarm's bar leaves the places to the rail.
    await expect(page.locator(".app-bar .places")).toHaveCount(0);

    await rail.locator("[data-open-panes]").click();
    await expect.poll(() => page.evaluate(() => location.hash)).toBe("");
    await expect(rail.locator("[data-open-panes]")).toHaveAttribute("aria-pressed", "true");
    expect((await tab(page)).id).toBe(t);
    expect(await active(page)).toBe(pane);

    // Sessions is a disclosure, distinct from destination selection.
    await expect(rail.locator("[data-open-sessions]")).toHaveAttribute("aria-expanded", "true");
    await expect(rail.locator("[data-open-sessions]")).not.toHaveAttribute("aria-pressed");
    // Sessions hides and shows the sidebar, and the device remembers.
    await rail.locator("[data-open-sessions]").click();
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    await expect(rail.locator("[data-open-sessions]")).toHaveAttribute("aria-expanded", "false");
    await page.reload();
    await expect(page.locator(".ws-rail")).toBeVisible();
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    await rail.locator("[data-open-sessions]").click();
    await expect(page.locator("[data-session-sidebar]")).toBeVisible();

    // Settings opens this device's settings.
    await rail.locator("[data-open-settings]").click();
    await expect(page.getByRole("menuitem", { name: "Appearance…" })).toBeVisible();
    await page.keyboard.press("Escape");
  });

  test("sidebar search: filters, arrows and Enter pick, an empty result says so", async ({ page }) => {
    await newWorkspace(page);
    await session(page, "alpha-search");
    await session(page, "alpine-search");
    await session(page, "zulu");
    const search = page.locator("[data-session-search]");

    await search.fill("search");
    await expect(page.locator(".ws-session")).toHaveCount(2);
    await expect(row(page, "zulu")).toHaveCount(0);
    // The first match is picked; Down moves to the second and wraps back.
    // Their order is by group, which follows what their fresh shells are
    // doing: wait until both have settled into one.
    await expect(page.locator("[data-session-group]")).toHaveCount(1);
    const picked = () => page.locator(".ws-session.hi").getAttribute("data-session-id").then(Number);
    const first = await picked();
    await search.press("ArrowDown");
    await expect.poll(picked).not.toBe(first);
    const second = await picked();
    await search.press("ArrowDown");
    await expect.poll(picked).toBe(first);
    await search.press("ArrowDown");
    await expect.poll(picked).toBe(second);
    await search.press("Enter");
    await expect.poll(() => shownSession(page)).toBe(second);
    // Enter hands the keys to the pane and clears the search.
    await expect(search).toHaveValue("");

    await search.fill("nothing-like-it");
    await expect(page.locator("[data-session-empty]")).toContainText("nothing-like-it");
    await expect(page.locator(".ws-session")).toHaveCount(0);
    await search.press("Escape");
    await expect(search).toHaveValue("");
    await expect(page.locator(".ws-session")).toHaveCount(4);
  });

  test("the connection chip says direct, then reconnecting while the link is down", async ({ page }) => {
    await newWorkspace(page);
    const chip = page.locator("[data-connection]");
    await expect(chip).toHaveAttribute("data-connection", "direct");
    // Drop the link and hold off the retry, so the state stays put.
    await page.evaluate(() => {
      const c = window.__arugula.client as unknown as { schedule: (f: () => void, d: number) => void; link?: { close(): void } };
      (window as unknown as { __retry: unknown }).__retry = c.schedule;
      c.schedule = () => {};
      c.link?.close();
    });
    await expect(chip).toHaveAttribute("data-connection", "reconnecting");
    await expect(chip).toHaveText("Reconnecting");
    await page.evaluate(() => {
      const c = window.__arugula.client as unknown as { schedule: unknown; connect(): void };
      c.schedule = (window as unknown as { __retry: unknown }).__retry;
      c.connect();
    });
    await expect(chip).toHaveAttribute("data-connection", "direct");
  });

  test("the old New workspace switch is forgotten; a saved palette stays, and Slate is the dark default", async ({ page, browser }) => {
    // Someone who picked Latte and had turned the new workspace off (M108–M112).
    await page.addInitScript(() => {
      try {
        localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "latte", finish: "flat", corners: "square", workspace: "classic" }));
      } catch {
        // No storage: nothing to forget.
      }
    });
    await newWorkspace(page);
    await expect(page.locator(".app-bar")).toHaveCount(0);
    await expect(page.locator("html")).toHaveAttribute("data-palette", "latte");
    const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("arugula-appearance") ?? "null"));
    expect(saved).toEqual({ palette: "latte", finish: "flat", corners: "square", density: "cozy" });
    // Appearance has no switch any more.
    await page.locator("[data-account-menu]").click();
    await page.getByRole("menuitem", { name: "Appearance…" }).click();
    await expect(page.locator("[data-appearance]")).toBeVisible();
    await expect(page.locator("[data-workspace-choice]")).toHaveCount(0);
    await page.getByRole("button", { name: "Use Control look" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-palette", "slate");
    await expect(page.locator("html")).toHaveAttribute("data-finish", "flat");
    await expect(page.locator("html")).toHaveAttribute("data-corners", "soft");
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem("arugula-appearance")!)))
      .toEqual({ palette: "slate", finish: "flat", corners: "soft", density: "cozy" });

    // Nobody's pick: Slate, in a dark scheme.
    const fresh = await browser.newPage({ viewport: { width: 1440, height: 900 }, colorScheme: "dark" });
    await fresh.goto(page.url());
    await expect(fresh.locator(".ws-rail")).toBeVisible();
    await expect(fresh.locator("html")).toHaveAttribute("data-palette", "slate");
    await fresh.close();
  });
});

/** Drop the page's link and hold off its retry; `back()` reconnects. */
async function dropLink(page: Page): Promise<() => Promise<void>> {
  await page.evaluate(() => {
    const c = window.__arugula.client as unknown as { schedule: (f: () => void, d: number) => void; link?: { close(): void } };
    (window as unknown as { __retry: unknown }).__retry = c.schedule;
    c.schedule = () => {};
    c.link?.close();
  });
  return async () => {
    await page.evaluate(() => {
      const c = window.__arugula.client as unknown as { schedule: unknown; connect(): void };
      c.schedule = (window as unknown as { __retry: unknown }).__retry;
      c.connect();
    });
  };
}

// M113 (#1028): each state the workspace can be in, made deliberate.
test.describe("states", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("an empty machine says what a session is, and makes one", async ({ page }) => {
    await newWorkspace(page);
    await page.evaluate(() => {
      const c = window.__arugula.client;
      for (const s of c.state!.sessions) c.intent({ op: "close_session", session: s.id });
    });
    const empty = page.locator("[data-no-sessions]");
    await expect(empty.locator(".empty-title")).toHaveText(/^No sessions on /);
    await expect(page.locator("[data-session-empty]")).toBeVisible();
    // Nothing that needs a session: no tab strip, toolbar or activity.
    await expect(page.locator(".ws-tabs, [data-pane-toolbar], [data-activity-toggle]")).toHaveCount(0);
    await empty.getByRole("button", { name: "New session" }).click();
    await expect.poll(() => panes(page)).toHaveLength(1);
    await expect(page.locator("[data-no-sessions]")).toHaveCount(0);
  });

  test("while the link is down nothing claims to be live, and what would change something says why", async ({ page }) => {
    await newWorkspace(page);
    const first = await active(page);
    await page.evaluate((pane) => window.__arugula.client.intent({ op: "split", pane, edge: "right" }), first);
    await expect.poll(() => panes(page)).toHaveLength(2);
    await expect(page.locator("[data-pane-status]").first()).toBeVisible();
    const back = await dropLink(page);

    await expect(page.locator(".app.ws.stale")).toBeVisible();
    await expect(page.locator("[data-stale-note]")).toContainText("nothing here is live");
    // No status a pane may no longer have, nor another window's size.
    await expect(page.locator("[data-pane-status]")).toHaveCount(0);
    await expect(page.locator("[data-sized-elsewhere]")).toHaveCount(0);
    await expect(page.locator(".ws-group-stale").first()).toHaveText(/last known/);
    for (const sel of [".new-tab", "[data-new-tab-more]", "[data-add-pane]", "[data-new-session]", ".ws-share", '[data-preset="grid"]']) {
      await expect(page.locator(sel).first()).toBeDisabled();
      await expect(page.locator(sel).first()).toHaveAttribute("title", /^Reconnecting to /);
    }

    await back();
    await expect(page.locator(".app.ws.stale")).toHaveCount(0);
    await expect(page.locator("[data-stale-note]")).toHaveCount(0);
    await expect(page.locator(".new-tab")).toBeEnabled();
  });

  test("a pane that can't be made says why, and + Pane stays open", async ({ page }) => {
    await newWorkspace(page);
    // The machine says no (as it would out of room, or to a bad path).
    await page.route("**/api/blocks", (route) =>
      route.request().method() === "POST"
        ? route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: "no room for another pane" }) })
        : route.continue(),
    );
    await page.locator("[data-add-pane]").click();
    const drawer = page.locator("[data-add-pane-drawer]");
    await drawer.locator('[data-kind="changes"]').click();
    await drawer.locator("[data-add-pane-create]").click();
    await expect(drawer.locator("[data-add-pane-error]")).toHaveText("No room for another pane");
    await expect(drawer.locator("[data-add-pane-error]")).toHaveAttribute("role", "alert");
    await expect(drawer).toBeVisible();
    expect(await panes(page)).toHaveLength(1);
  });

  test("20 tabs: the strip scrolls, keeps the shown tab and + in sight; long names are cut, whole in their tooltip", async ({ page }) => {
    await newWorkspace(page);
    const long = "a-session-whose-name-goes-on-and-on-past-any-sidebar-width";
    const s = await session(page, long);
    await page.evaluate((s) => window.__arugula.client.selectSession(s), s);
    for (let i = 1; i < 20; i++) {
      // One at a time: a burst of new terminals runs headless Chrome out of
      // WebGL contexts.
      await page.evaluate((s) => window.__arugula.client.intent({ op: "new_tab", session: s, from_pane: null }), s);
      await expect.poll(() => tabsInSession(page)).toHaveLength(i + 1);
    }
    const ids = await tabsInSession(page);
    await page.evaluate((t) => window.__arugula.client.selectTab(t), ids[19]);
    const strip = page.locator(".ws-tabs .tabbar");
    const box = (await strip.boundingBox())!;
    await expect(strip).toHaveAttribute("data-overflow", /start|both/);
    await expect
      .poll(async () => {
        const t = (await page.locator(`[data-tab-id="${ids[19]}"]`).boundingBox())!;
        return t.x >= box.x - 1 && t.x + t.width <= box.x + box.width + 1;
      })
      .toBe(true);
    // + and ▾ beside the tabs, on screen.
    for (const sel of [".ws-tabs .new-tab", ".ws-tabs [data-new-tab-more]"]) {
      const b = (await page.locator(sel).boundingBox())!;
      expect(b.x + b.width).toBeLessThanOrEqual(1440);
    }
    // The wheel scrolls it sideways.
    const before = await strip.evaluate((el) => el.scrollLeft);
    await strip.hover();
    await page.mouse.wheel(0, -400);
    await expect.poll(() => strip.evaluate((el) => el.scrollLeft)).toBeLessThan(before);

    // The long name: cut with an ellipsis in the crumb and the row, whole
    // where the pointer rests.
    const crumb = page.locator(".ws-crumb-name");
    await expect(crumb).toHaveText(long);
    expect(await crumb.evaluate((el) => el.scrollWidth > el.clientWidth && getComputedStyle(el).textOverflow === "ellipsis")).toBe(true);
    await expect(page.locator(".ws-crumb")).toHaveAttribute("title", new RegExp(`^${long}`));
    await expect(row(page, long).locator(".ws-session-main")).toHaveAttribute("title", new RegExp(`^${long}`));
  });
});

// M113 (#1028): keyboard, names, contrast and motion.
test.describe("accessibility", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("every icon button is named, with a tooltip", async ({ page }) => {
    await newWorkspace(page);
    const first = await active(page);
    await page.evaluate((pane) => window.__arugula.client.intent({ op: "split", pane, edge: "right" }), first);
    await expect.poll(() => panes(page)).toHaveLength(2);
    const unnamed = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>(".app.ws button")]
        .filter((b) => b.offsetParent && !/[A-Za-z0-9]/.test(b.textContent ?? ""))
        .filter((b) => !b.getAttribute("aria-label") || !b.title)
        .map((b) => b.outerHTML.slice(0, 120)),
    );
    expect(unnamed).toEqual([]);
    // And the ones with a key say it.
    await expect(page.locator("[data-pane-expand]").first()).toHaveAttribute("title", /\(Ctrl\+Shift\+Enter\)$/);
    await expect(page.locator("[data-add-pane]")).toHaveAttribute("title", /\(Ctrl\+Shift\+E\)$/);
    await expect(page.locator("[data-open-palette]")).toHaveAttribute("title", /\(Ctrl\+Shift\+P\)$/);
  });

  test("from the keyboard: out of the terminal by the palette, along the tabs, and back", async ({ page }) => {
    await newWorkspace(page);
    await page.locator(".ws-tabs .new-tab").click();
    await expect.poll(() => tabsInSession(page)).toHaveLength(2);
    const [t1, t2] = await tabsInSession(page);
    await expect.poll(() => tab(page).then((t) => t.id)).toBe(t2);

    // The terminal has the keys; the palette moves them to the tabs.
    await page.keyboard.press("Control+Shift+KeyP");
    await expect(page.getByRole("textbox", { name: "Command", exact: true })).toBeFocused();
    await page.keyboard.type("keyboard to the tabs");
    await expect(page.locator(".palette-row.selected")).toContainText("Move the keyboard to the tabs");
    await page.keyboard.press("Enter");
    const shown = page.locator(`[data-tab-id="${t2}"]`);
    await expect(shown).toBeFocused();
    // Visibly.
    expect(await shown.evaluate((el) => getComputedStyle(el).outlineStyle)).not.toBe("none");
    // Left goes to the first tab without showing it; Enter shows it, and its
    // pane takes the keys.
    await page.keyboard.press("ArrowLeft");
    await expect(page.locator(`[data-tab-id="${t1}"]`)).toBeFocused();
    expect((await tab(page)).id).toBe(t2);
    await page.keyboard.press("Enter");
    await expect.poll(() => tab(page).then((t) => t.id)).toBe(t1);
    // A state change doesn't take the keys from a tab a keyboard is on.
    await page.keyboard.press("Control+Shift+KeyP");
    await expect(page.getByRole("textbox", { name: "Command", exact: true })).toBeFocused();
    await page.keyboard.type("keyboard to the tabs");
    await expect(page.locator(".palette-row.selected")).toContainText("Move the keyboard to the tabs");
    await page.keyboard.press("Enter");
    await expect(page.locator(`[data-tab-id="${t1}"]`)).toBeFocused();
    await page.evaluate((t) => window.__arugula.client.intent({ op: "rename_tab", tab: t, name: "renamed" }), t2);
    await expect(page.locator(`[data-tab-id="${t2}"]`)).toContainText("renamed");
    await expect(page.locator(`[data-tab-id="${t1}"]`)).toBeFocused();
    // And the sidebar: the palette puts the keys in its search.
    await page.keyboard.press("Control+Shift+KeyP");
    await expect(page.getByRole("textbox", { name: "Command", exact: true })).toBeFocused();
    await page.keyboard.type("keyboard to the sessions");
    await expect(page.locator(".palette-row.selected")).toContainText("Move the keyboard to the sessions");
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-session-search]")).toBeFocused();
  });

  test("on every palette the chrome's words read at 4.5:1", async ({ page }) => {
    await newWorkspace(page);
    await page.locator("[data-account-menu]").click();
    await page.getByRole("menuitem", { name: "Appearance…" }).click();
    const ids = await page.locator("[data-palette-choice]").evaluateAll((els) => els.map((e) => e.getAttribute("data-palette-choice")!));
    expect(ids.length).toBeGreaterThan(5);
    for (const id of ids) {
      await page.locator(`[data-palette-choice="${id}"]`).click();
      const worst = await page.evaluate(() => {
        const css = getComputedStyle(document.documentElement);
        const v = (k: string) => css.getPropertyValue(`--${k}`).trim();
        const lum = (hex: string) => {
          const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
          return 0.2126 * r + 0.7152 * g + 0.0722 * b;
        };
        const ratio = (a: string, b: string) => {
          const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
          return (x + 0.05) / (y + 0.05);
        };
        let min = Infinity;
        let at = "";
        for (const t of ["fg", "dim", "subtle", "accent", "warn", "ok", "danger", "info"])
          for (const b of ["bg", "surface", "raised"]) {
            const r = ratio(v(t), v(b));
            if (r < min) [min, at] = [r, `${t} on ${b}`];
          }
        return { min, at };
      });
      expect(worst.min, `${id}: ${worst.at}`).toBeGreaterThanOrEqual(4.5);
    }
    // Back to nothing picked.
    await page.getByRole("button", { name: "Reset" }).click();
  });

  test("terminal text contrasts with explicit program backgrounds after a palette change", async ({ page }) => {
    await newWorkspace(page);
    await page.locator("[data-account-menu]").click();
    await page.getByRole("menuitem", { name: "Appearance…" }).click();
    for (const palette of ["latte", "slate"]) {
      await page.locator(`[data-palette-choice="${palette}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("data-palette", palette);
      await page.evaluate(async () => {
        const v = window.__arugula.client.viewOf(window.__arugula.client.active()!)!;
        v.setVisible(false); // Inspect the DOM renderer's actual cell colors.
        await new Promise<void>((resolve) => v.term.write("\x1b[2J\x1b[H\x1b[0;48;2;56;56;72mCOMPOSER\x1b[0m\r\n\x1b[38;2;166;227;161;48;2;239;241;245mSTATUS\x1b[0m", resolve));
      });
      for (const text of ["COMPOSER", "STATUS"]) {
        const cell = page.locator(".xterm-rows span").filter({ hasText: text });
        await expect(cell).toBeVisible();
        // The renderer replaces a row's spans as it redraws: a span it let go
        // of has no computed colors, so read again until one has.
        await expect.poll(() => cell.first().evaluate((el) => {
          const css = getComputedStyle(el);
          const lum = (color: string) => (color.match(/\d+/g) ?? []).slice(0, 3).map(Number).map((c) => {
            const n = c / 255;
            return n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4;
          }).reduce((sum, n, i) => sum + n * [0.2126, 0.7152, 0.0722][i], 0);
          if (!css.color || !css.backgroundColor) return 0;
          const a = lum(css.color), b = lum(css.backgroundColor);
          return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
        }).catch(() => 0), { message: `${palette} ${text}` }).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  test("less motion when asked: nothing animates, and the cursor doesn't blink", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await newWorkspace(page);
    const pane = await active(page);
    const blink = await page.evaluate((p) => {
      const v = window.__arugula.client.viewOf(p) as unknown as { term: { options: { cursorBlink: boolean } } };
      return v.term.options.cursorBlink;
    }, pane);
    expect(blink).toBe(false);
    const duration = await page.evaluate(() => {
      const el = document.createElement("span");
      el.className = "att needs_input";
      document.querySelector(".ws-foot")!.append(el);
      const d = getComputedStyle(el).animationDuration;
      el.remove();
      return parseFloat(d);
    });
    expect(duration).toBeLessThan(0.01);
  });
});

test.describe("on a narrow window", () => {
  test.use({ viewport: { width: 1000, height: 700 } });

  test("the sidebar folds into the rail and opens over the panes", async ({ page }) => {
    await newWorkspace(page);
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    const before = await page.locator(".main").boundingBox();
    const beta = await session(page, "narrow-beta");
    await page.locator(".ws-rail [data-open-sessions]").click();
    await expect(page.locator("[data-session-sidebar].overlay")).toBeVisible();
    // Over the panes: they keep their room.
    expect((await page.locator(".main").boundingBox())?.width).toBe(before?.width);
    const firstName = await page.evaluate(() => window.__arugula.client.state!.sessions[0].name);
    await row(page, firstName).locator(".ws-session-main").click();
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    expect(await shownSession(page)).not.toBe(beta);
  });
});
