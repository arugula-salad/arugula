// What a person does in the app's window, by what's on screen: the host
// menu's account items, switching machines, teams and their invite links,
// moving a machine with In …. Shared by the J2 journeys (#551).

import { expect, type Page } from "@playwright/test";
import type { App } from "./app";

/** The host menu's account items: Teams…, Devices and machines… */
export async function account(p: Page, item: "Teams…" | "Devices and machines…") {
  await p.getByTitle("Hosts").click();
  await p.getByRole("menuitem", { name: item }).click();
}

/** Switch the window to `machine` from the host menu. */
export async function show(p: Page, machine: string) {
  if ((await p.getByTitle("Hosts").innerText()).includes(machine)) return;
  await p.getByTitle("Hosts").click();
  await p.getByRole("menuitem", { name: new RegExp(`\\b${machine}\\b`) }).click();
  await expect(p.getByTitle("Hosts")).toContainText(machine, { timeout: 20_000 });
}

/** Make a team and an invite link to it, in the Teams panel. */
export async function makeTeam(p: Page, name: string): Promise<string> {
  await account(p, "Teams…");
  await p.getByRole("textbox", { name: "Team name" }).fill(name);
  await p.getByRole("button", { name: "Make a team" }).click();
  await p.getByRole("button", { name: "Make a link" }).last().click();
  const link = /\S+#p?invite=\S+/.exec(
    await p
      .getByText(/#p?invite=/)
      .last()
      .innerText(),
  )![0];
  await p.getByRole("button", { name: "Done" }).click();
  return link;
}

/** Open an invite link in the person's browser and join. */
export async function join(app: App, link: string, team: string) {
  const tab = await app.person.browser.newPage();
  await tab.goto(link);
  await tab.getByRole("button", { name: `Join ${team}` }).click();
  await expect(tab.getByText(`You're in ${team}`)).toBeVisible({ timeout: 20_000 });
  return tab;
}

/** Devices and machines…: put `machine` in `team` with In …, Move. */
export async function moveInto(p: Page, machine: string, team: string) {
  await account(p, "Devices and machines…");
  const row = p
    .getByRole("listitem")
    .filter({ hasText: machine })
    .filter({ has: p.getByRole("combobox") });
  await row.getByRole("combobox").selectOption({ label: team });
  await row.getByRole("button", { name: "Move", exact: true }).click();
  await expect(row.getByRole("button", { name: "Move", exact: true })).toBeHidden({ timeout: 20_000 });
  await p.getByRole("button", { name: "Done" }).click();
}
