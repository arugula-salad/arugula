// Comments on a Doc (#1258): a click on a paragraph opens a box in the
// margin beside it, Save posts a message to the Doc's thread whose text links
// to the section, quotes the paragraph and mentions the agent session that
// wrote the Doc, and the comment then sits in the margin at that paragraph.
// On the platform's Doc page (#971's fixture page) and on the Doc's embed
// page for the app's web view (shell/doc-embed.tsx), where a link goes to the
// app as an intent and a phone's width lists the comments under the Doc. The
// platform's answers, the thread's included, are this spec's.
import { expect, test } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { answers, comment, DOC, embed, expectAnchored, PARA, SITE, told } from "./doc-comments-steps";

test("on the Doc's page, a comment on a paragraph is posted to its thread for the agent, and sits beside it", async ({ page }) => {
  const posted = await answers(page);
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(`/p/${SITE}/docs/${DOC}`);
  await expect(page.locator("[data-doc-margin]")).toContainText("geek/docs-agent sees it");
  await comment(page, "Even photos?");
  expectAnchored(posted, `${new URL(page.url()).origin}/p/${SITE}/docs/${DOC}`);
  const card = page.locator("[data-doc-comment='1']");
  await expect(card).toContainText("Even photos?");
  await expect(card).not.toContainText("On “What it does”");
  await expect(card).toHaveAttribute("data-anchored", "");
  // Beside its paragraph: their tops line up.
  const para = page.locator(".doc-body p", { hasText: "Replies you send" });
  await expect(para).toHaveClass(/doc-commented-p/);
  const [p, c] = [await para.boundingBox(), await card.boundingBox()];
  expect(Math.abs(p!.y - c!.y)).toBeLessThan(4);
  expect(c!.x).toBeGreaterThan(p!.x + p!.width);
});

test("the embed page is the Doc alone, its links go to the app, and a comment saves", async ({ page }) => {
  const posted = await answers(page);
  await embed(page, 1024);
  await expect(page.locator(".rail, .platform-bar, [data-first-run]")).toHaveCount(0);
  await expect.poll(() => told(page)).toContainEqual({ kind: "ready", title: "Chat on a bad network" });
  await page.getByRole("link", { name: "the board" }).click();
  await expect.poll(() => told(page)).toContainEqual({ kind: "board", project: SITE, id: "brd_aaaa0003", url: `https://app.test/p/${SITE}/boards/brd_aaaa0003` });
  expect(page.url()).toContain(`/docs/${DOC}`);
  await comment(page, "Even photos?");
  expectAnchored(posted, `https://app.test/p/${SITE}/docs/${DOC}`);
  await expect(page.locator("[data-doc-margin] [data-doc-comment='1']")).toContainText("Even photos?");
});

test("on a phone's width the comments are listed under the Doc, and the box comes up from the bottom", async ({ page }) => {
  const posted = await answers(page);
  await embed(page, 390);
  await expect(page.locator("[data-doc-margin]")).toHaveCount(0);
  await comment(page, "Even photos?");
  expect(posted).toHaveLength(1);
  const list = page.locator("[data-doc-comment-list]");
  await expect(list).toContainText("Comments (1)");
  await expect(list.locator("[data-doc-comment='1']")).toContainText(PARA);
});
