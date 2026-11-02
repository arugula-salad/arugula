// #508: a prototype's bridge to its parent: copy a value (only on a click),
// and keep settings per viewer, per Doc and section. The frame has an opaque
// origin; `window.arugula` (runner.html's shim) asks the Doc, which answers
// only the frame it made.
//
// The traps: a copy tested with a synthetic message passes even with no
// gesture check, so the copy here is a real click inside the frame, and a
// copy from a timer with no click must be refused; and the store's key
// must name the Doc, or two Docs with a section `s1` share sliders.

import { expect, test, type Page } from "@playwright/test";
import { fill, newDoc, type Post } from "./doc-steps";
import { panes, reset } from "./helpers";

test.use({ permissions: ["clipboard-read", "clipboard-write"] });

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

/** A slider that keeps its value, a button that copies, and a copy that a
 * timer tries, long after any click. */
const page1 = `\`\`\`prototype height=240 Bridge
<!doctype html>
<body>
<input id="s" type="range" min="0" max="100" value="50">
<button id="copy">Copy values</button>
<p id="out">waiting</p>
<p id="idle">waiting</p>
<script>
const s = document.getElementById("s");
const out = document.getElementById("out");
arugula.store.all().then((all) => { if (typeof all.s === "number") s.value = String(all.s); out.textContent = "ready"; });
s.addEventListener("input", () => arugula.store.set("s", Number(s.value)));
document.getElementById("copy").addEventListener("click", () => {
  arugula.copy("slider=" + s.value).then(() => (out.textContent = "copied"), (e) => (out.textContent = "refused: " + e.message));
});
setTimeout(() => {
  arugula.copy("no gesture").then(() => (document.getElementById("idle").textContent = "copied"), (e) => (document.getElementById("idle").textContent = "refused: " + e.message));
}, 8000);
</script>
</body>
\`\`\``;

const frameOf = (page: Page, doc: number) => page.frameLocator(`[data-doc-block="${doc}"] [data-prototype-frame]`);

test("a click in the frame copies to the clipboard, and a copy with no click is refused", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await page.evaluate(() => navigator.clipboard.writeText("untouched"));
  await fill(post(page), doc, "s1", page1);
  const inside = frameOf(page, doc);
  // Playwright's own evaluates and polls count as a gesture, and one lasts
  // about five seconds. The frame's timer fires at 8s and nothing touches the
  // page until it has: it really has no click.
  await page.waitForTimeout(10_000);
  await expect(inside.locator("#idle")).toHaveText("refused: needs a click", { timeout: 15_000 });
  expect(await page.evaluate(() => navigator.clipboard.readText()), "no click, no copy").toBe("untouched");

  await inside.locator("#copy").click();
  await expect(inside.locator("#out")).toHaveText("copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("slider=50");
  await expect(page.locator(`[data-doc-block="${doc}"] [data-prototype-copied]`)).toHaveText("Copied");
  await expect(page.locator("[data-prototype-copied]")).toHaveCount(0, { timeout: 5000 });
});

test("a kept slider comes back after a reload, and another Doc doesn't see it", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  const other = await newDoc(post(page), term, "Other");
  await fill(post(page), doc, "s1", page1);
  await fill(post(page), other, "s1", page1);
  const a = frameOf(page, doc);
  const b = frameOf(page, other);
  await expect(a.locator("#out")).toHaveText("ready", { timeout: 15_000 });
  await expect(b.locator("#out")).toHaveText("ready", { timeout: 15_000 });
  await a.locator("#s").fill("73");
  await expect.poll(() => page.evaluate((d) => Object.keys(localStorage).filter((k) => k.startsWith("arugula.proto.")).sort().join(), doc)).toBe(`arugula.proto.${doc}.s1`);
  await expect(b.locator("#s")).toHaveValue("50");

  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null)).toBe(true);
  await expect(frameOf(page, doc).locator("#out")).toHaveText("ready", { timeout: 15_000 });
  await expect(frameOf(page, doc).locator("#s")).toHaveValue("73");
  await expect(frameOf(page, other).locator("#out")).toHaveText("ready", { timeout: 15_000 });
  await expect(frameOf(page, other).locator("#s")).toHaveValue("50");
});

test("a message from another window gets no answer, and the caps are enforced", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fill(post(page), doc, "s1", page1);
  const inside = frameOf(page, doc);
  await expect(inside.locator("#out")).toHaveText("ready", { timeout: 15_000 });

  // A sibling frame (not the Doc's) and the page itself ask the Doc: no
  // reply reaches the sibling, and nothing is written.
  const heard = await page.evaluate(
    (d) =>
      new Promise<string[]>((done) => {
        const got: string[] = [];
        const f = document.createElement("iframe");
        f.setAttribute("sandbox", "allow-scripts");
        f.srcdoc = `<script>addEventListener("message", (e) => parent.postMessage({ heard: JSON.stringify(e.data) }, "*"));
          parent.postMessage({ type: "arugula.store.set", id: 1, key: "evil", value: 1 }, "*");
          parent.postMessage({ type: "arugula.store.all", id: 2 }, "*");<\/script>`;
        addEventListener("message", (e) => {
          if (e.source === f.contentWindow && (e.data as { heard?: string }).heard) got.push((e.data as { heard: string }).heard);
        });
        document.body.append(f);
        setTimeout(() => done(got), 1500);
        void d;
      }),
    doc,
  );
  expect(heard, "the sibling heard nothing").toEqual([]);
  expect(await page.evaluate((d) => localStorage.getItem(`arugula.proto.${d}.s1`), doc)).toBeNull();

  // Oversized keys, values and sections, and unknown shapes.
  const r = await inside.locator("html").evaluate(async () => {
    const a = (window as unknown as { arugula: { store: { set(k: string, v: unknown): Promise<unknown> }; copy(t: string): Promise<unknown> } }).arugula;
    const err = (p: Promise<unknown>) => p.then(() => "ok", (e: Error) => e.message);
    const out: Record<string, string> = {};
    out.longKey = await err(a.store.set("k".repeat(129), 1));
    out.bigValue = await err(a.store.set("big", "v".repeat(8 * 1024)));
    out.fits = await err(a.store.set("fits", "v".repeat(8 * 1024 - 2)));
    let full = "ok";
    for (let i = 0; i < 10 && full === "ok"; i++) full = await err(a.store.set("f" + i, "v".repeat(8 * 1024 - 2)));
    out.full = full;
    out.tooLong = await err(a.copy("x".repeat(16 * 1024 + 1)));
    return out;
  });
  expect(r.longKey).toBe("key too long");
  expect(r.bigValue).toBe("value too big");
  expect(r.fits).toBe("ok");
  expect(r.full).toBe("store full");
  expect(r.tooLong).toBe("text too long");
});
