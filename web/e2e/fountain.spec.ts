// Direct Managoat execution moved to the account's Cloud agents panel.
import { expect, test } from "@playwright/test";
import { paneEl, panes, reset } from "./helpers";

test("local agent creation keeps retired choices out of the unified launcher", async ({ page }) => {
  await reset(page);
  await page.evaluate(() => localStorage.setItem("arugula.agent", JSON.stringify({kind:"managoat", fountain:"old-agent"})));
  const [term] = await panes(page);
  await paneEl(page, term).click({button:"right", position:{x:60,y:60}});
  await expect(page.getByRole("menuitem", {name:"Managoat…", exact:true})).toHaveCount(0);
  await page.getByRole("menuitem", {name:"Start an agent…", exact:true}).click();
  const dialog = page.getByRole("dialog", {name:"Start an agent"});
  await expect(dialog.locator("select[name=agent]")).toHaveValue("claude");
  await expect(dialog.locator('option[value="managoat"]')).toHaveCount(0);
  await expect(dialog.getByLabel("Run on")).toHaveValue("local");
  await dialog.getByRole("button", {name:"Cancel", exact:true}).click();
  await page.evaluate(() => localStorage.removeItem("arugula.agent"));
});
