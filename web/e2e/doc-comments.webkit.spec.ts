// Comments on a Doc (#1258) in WebKit, the engine of the app's web view: the
// embed page's checks from doc-comments.spec.ts (a link goes to the app as an
// intent, a comment saves, a phone's width lists them under the Doc).
import { expect, test } from "@playwright/test";
import { answers, comment, DOC, embed, expectAnchored, PARA, SITE, told } from "./doc-comments-steps";

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
