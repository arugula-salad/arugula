import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { bring, newDoc, png, type Post } from "./doc-steps";
import { open, paneEl, panes, reset } from "./helpers";
import { pixel7 } from "./phones";

const fake = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));
const post = (page: Page): Post => async (path, data) => {
  const res = await page.request.post(path, { data });
  expect(res.ok(), await res.text()).toBe(true);
  return res.json();
};
async function state(page: Page, block: number) {
  return (await (await page.request.get(`/api/blocks/${block}`)).json()).state;
}
async function setup(page: Page) {
  await reset(page);
  const [term] = await panes(page);
  const owner = (await post(page)("/api/blocks", { type: "agent", config: { agent: "acp", command: ["python3", fake], cwd: "/tmp", prompt: "hello" }, split: term, local: true })).block as number;
  const other = (await post(page)("/api/blocks", { type: "agent", config: { agent: "acp", command: ["python3", fake], cwd: "/tmp", prompt: "hello" }, split: term, local: true })).block as number;
  const doc = await newDoc(post(page), term);
  await post(page)(`/api/blocks/${doc}/call/doc_checklist`, { section: "s1", items: [{ id: "phone", text: "Phone layout fits", owner: { role: "frontend", pane: owner } }] });
  await bring(page, doc);
  return { doc, owner, other, row: page.locator(`[data-doc-block="${doc}"] [data-doc-test=phone]`) };
}

test("saved pass and failure results reach only the assigned agent", async ({ page }) => {
  const { doc, owner, other, row } = await setup(page);
  // The controlled checkbox reflects the saved daemon state, asynchronously.
  await row.getByRole("checkbox", { name: "Phone layout fits" }).click();
  await expect.poll(async () => (await state(page, doc)).entries[0].tasks[0].status).toBe("passed");
  await expect(row.getByRole("checkbox")).toBeChecked();
  await row.getByRole("button", { name: "Report a failure" }).click();
  await row.getByRole("textbox", { name: "Failure note for Phone layout fits" }).fill("The action buttons overlap on the phone.");
  await row.getByRole("button", { name: "Report failure to frontend" }).click();
  await expect(row).toHaveAttribute("data-test-status", "failed");
  await expect(row).toContainText("Sent to owner", { timeout: 15_000 });
  await expect.poll(async () => JSON.stringify(await state(page, owner))).toContain("The action buttons overlap on the phone.");
  expect(JSON.stringify(await state(page, other))).not.toContain("The action buttons overlap on the phone.");
  await open(page);
  await bring(page, doc);
  await expect(row).toContainText("The action buttons overlap on the phone.");
  await expect(row.getByRole("checkbox")).not.toBeChecked();
  // Neither an agent's CLI nor a stale second device can overwrite it.
  const agent = await page.request.post(`/api/blocks/${doc}/call/doc_feedback`, { headers: { "x-arugula-agent": "1" }, data: { section: "s1", item: "phone", revision: 2, status: "passed" } });
  expect(agent.status()).toBe(403);
  const stale = await page.request.post(`/api/blocks/${doc}/call/doc_feedback`, { data: { section: "s1", item: "phone", revision: 0, status: "passed" } });
  expect(stale.ok()).toBe(false);
  await expect(row).toHaveAttribute("data-test-status", "failed");
  await row.getByRole("checkbox").click();
  await expect(row.getByRole("checkbox")).toBeChecked();
  await expect.poll(async () => JSON.stringify(await state(page, owner))).toContain("Test result updated: passed");
});

test("a draft failure cannot overwrite a result received while editing", async ({ page }) => {
  const { doc, row } = await setup(page);
  await row.getByRole("button", { name: "Report a failure" }).click();
  const draft = row.getByRole("textbox", { name: "Failure note for Phone layout fits" });
  await draft.fill("My unsent report.");
  await post(page)(`/api/blocks/${doc}/call/doc_feedback`, { section: "s1", item: "phone", revision: 0, status: "failed", note: "Another device's report." });
  await expect(row.locator("blockquote")).toHaveText("Another device's report.");
  await row.getByRole("button", { name: "Report failure to frontend" }).click();
  await expect(row.getByRole("alert")).toContainText("another device");
  expect((await state(page, doc)).entries[0].tasks[0].note).toBe("Another device's report.");
  await expect(draft).toHaveValue("My unsent report.");
  await row.getByRole("button", { name: "Review latest result and keep my draft" }).click();
  await row.getByRole("button", { name: "Report failure to frontend" }).click();
  await expect(row.locator("blockquote")).toHaveText("My unsent report.");
});

test.describe("on a phone", () => {
  test.use(pixel7);
  test("a failure screenshot is saved and attached to the owner's prompt", async ({ page }) => {
    const { doc, owner, row } = await setup(page);
    await row.getByRole("button", { name: "Report a failure" }).click();
    await row.locator('input[type="file"]').setInputFiles({ name: "failure.png", mimeType: "image/png", buffer: png(8, 6) });
    await row.getByRole("button", { name: "Report failure to frontend" }).click();
    await expect(row).toContainText("Sent to owner", { timeout: 15_000 });
    await expect(row.getByRole("img", { name: "Test screenshot" })).toBeVisible();
    if (process.env.CHECKLIST_SHOT) await page.locator(`[data-doc-block="${doc}"]`).screenshot({ path: process.env.CHECKLIST_SHOT });
    await bring(page, owner);
    const report = paneEl(page, owner).locator(".agent-user").filter({ hasText: "Test failure for frontend" });
    await expect(report.locator(".agent-image img")).toHaveCount(1, { timeout: 15_000 });
  });
});
