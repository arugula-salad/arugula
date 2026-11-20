// What doc-comments.spec.ts and its WebKit twin share (#1258): the
// platform's answers for one Doc and its thread, the embed page with the
// app's message handler, and making a comment.
import { expect, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

export const SITE = "prj_aaaa0002";
export const DOC = "pdc_aaaa0001";
const ROOM = "ch_doc0001";
export const PARA = "Replies you send queue on the device and go when the network is back.";

interface Posted {
  text: string;
}

/** The platform's answers for one Doc an agent session wrote, and its thread; what's posted is kept. */
export async function answers(page: Page): Promise<Posted[]> {
  const posted: Posted[] = [];
  const now = Date.now();
  const doc = {
    id: DOC,
    project: SITE,
    title: "Chat on a bad network",
    author: "acct_a",
    agent: { machine: "m_geek", machine_name: "geek", session: "Docs Agent" },
    sections: 2,
    written: 2,
    rev: 1,
    created_at: now,
    updated_at: now,
  };
  const view = {
    doc,
    sections: [
      { id: "s1", heading: "Why", intent: "", body: "Jake reads chat on the train.", rev: 1 },
      { id: "s2", heading: "What it does", intent: "", body: `${PARA}\n\nAgents keep working while you're offline. See [the board](https://app.test/p/${SITE}/boards/brd_aaaa0003).`, rev: 1 },
    ],
    assets: [],
    writer: false,
    deleter: false,
    link: null,
  };
  const room = {
    id: ROOM,
    kind: "object",
    team: null,
    name: doc.title,
    machine: null,
    machine_name: null,
    target: DOC,
    project: SITE,
    with: null,
    others: [],
    private: false,
    last: 0,
    read: 0,
    unread: 0,
    mentions: 0,
    wants: 0,
    muted: false,
    level: "mentions",
    level_own: false,
    archived: false,
    writable: true,
    topic: "",
    description: "",
    pins: 0,
    topic_writable: false,
  };
  const messages = () =>
    posted.map((p, i) => ({
      seq: i + 1,
      at: now,
      author: "acct_a",
      name: "Sam",
      text: p.text,
      quote: null,
      pane: null,
      mentions: [],
      edited: null,
      deleted: null,
      imported: null,
      agent: null,
      reactions: [],
      files: [],
      pinned: false,
      also: false,
    }));
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
  await page.route(new RegExp(`/api/projects/[^/]+/docs/${DOC}$`), (r) => json(r, view));
  await page.route(new RegExp(`/api/projects/[^/]+/docs/${DOC}/boards$`), (r) => json(r, { boards: [] }));
  await page.route(/\/api\/chat\/targets$/, (r) => json(r, room));
  await page.route(new RegExp(`/api/chat/rooms/${ROOM}/messages(\\?.*)?$`), (r) => {
    if (r.request().method() === "POST") {
      posted.push(r.request().postDataJSON() as Posted);
      return json(r, { seq: posted.length, mentions: [], unreached: [] });
    }
    const after = Number(new URL(r.request().url()).searchParams.get("after") ?? 0);
    return json(r, { room, messages: messages().filter((m) => m.seq > after), more: false });
  });
  return posted;
}

/** Click the paragraph, write a comment and save it; the box goes. */
export async function comment(page: Page, said: string) {
  await page.locator(".doc-body p", { hasText: "Replies you send" }).click();
  const box = page.locator("[data-doc-comment-box]");
  await expect(box).toBeVisible();
  await box.locator("[data-doc-comment-input]").fill(said);
  await box.locator("[data-doc-comment-save]").click();
  await expect(box).toHaveCount(0);
}

export function expectAnchored(posted: Posted[], url: string) {
  expect(posted).toHaveLength(1);
  expect(posted[0]!.text).toBe(`[On “What it does”](${url}#s2)\n> ${PARA}\n\nEven photos?\n\n@geek/docs-agent`);
}

/** The embed page, with the app's message handler: what it's told is kept on `window.__told`. */
export async function embed(page: Page, width: number) {
  const body = await fixturePage("../src/shell/doc-embed.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.addInitScript(() => {
    const told: unknown[] = [];
    (window as unknown as { __told: unknown[] }).__told = told;
    (window as unknown as { webkit: unknown }).webkit = { messageHandlers: { arugula: { postMessage: (m: unknown) => told.push(m) } } };
  });
  await page.setViewportSize({ width, height: 900 });
  await page.goto(`/embed/p/${SITE}/docs/${DOC}`);
  await expect(page.locator("[data-embed] .doc-title")).toHaveText("Chat on a bad network");
}

export const told = (page: Page) => page.evaluate(() => (window as unknown as { __told: { kind: string }[] }).__told);

