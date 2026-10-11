// J2a (#551): two friends, each through J1 on their own Mac first, link
// up by sharing a session: Sam shares one with Riley, who isn't on a
// team with him; Riley accepts, finds Sam's machine, asks to drive, Sam
// allows it, and what Riley types shows in Sam's terminal, on Sam's
// screen. What only two people meet: what they must tell each other
// outside Arugula (a login; Riley's key is pinned on first use, #763),
// the offer to accept, finding the shared machine, and the ask to drive a
// pane on someone's own machine.
//
// Each person's first run is its own report (J2a-setup-sam, -riley), so a
// signup failure there can't hide a linking failure here. Rules as in
// journey-j1.spec.ts: only what's on screen, each step's prompt named.
//   just journey j2a

import { expect, test, type Page } from "@playwright/test";
import type { App } from "./journey/app";
import { clickTerminal, onboarded, screenText } from "./journey/first-run";
import { Journey, type Surface } from "./journey/record";
import { World } from "./journey/world";
import { closeContexts } from "./helpers";

test.afterAll(closeContexts);
test.use({ actionTimeout: 20_000 });

const world = new World();
test.beforeAll(() => world.start());
test.afterAll(() => world.stop());

test("J2a: two friends link up by sharing a session; the friend types in the owner's terminal", async ({ browser }, info) => {
  test.setTimeout(420_000);
  const sam = await onboarded(world, browser, info, "J2a", "sam", "sammac");
  const riley = await onboarded(world, browser, info, "J2a", "riley", "rileymac");
  const j = new Journey("J2a", "two friends link up: share a session, the friend drives it", info, ["sam", "riley"]);
  const at = (who: App) => () => who.window;
  const step = (
    id: string,
    title: string,
    actor: "sam" | "riley",
    surface: Surface,
    page: () => Page,
    prompt: (p: Page) => import("@playwright/test").Locator | null,
    act: (p: Page) => Promise<void>,
    why?: string,
  ) => j.step({ id, title, actor, surface, page, prompt: prompt(page()), why }, () => act(page()));
  let pane = "";
  const marker = "hi-from-riley-$((6*7))";

  try {
    await j.step(
      {
        id: "agree",
        title: "Riley tells Sam their Arugula login",
        actor: "riley",
        surface: "message",
        prompt: null,
        own: "the friends agree to link up; Riley sends their login (riley)",
      },
      async () => {},
    );

    await step(
      "open-share",
      "Sam opens Share session…",
      "sam",
      "app",
      at(sam),
      (p) => p.getByRole("button", { name: /share/i }),
      async (p) => {
        // On the bar, beside the session's name.
        await p.getByRole("button", { name: /share/i }).first().click();
      },
    );

    await step(
      "share-who",
      "Sam enters Riley's login, as someone who can drive, and shares",
      "sam",
      "app",
      at(sam),
      (p) => p.getByRole("textbox", { name: "Who" }),
      async (p) => {
        await p.getByRole("textbox", { name: "Who" }).fill("riley");
        await p.getByRole("combobox", { name: "Role" }).selectOption({ label: "drives" });
        await p.getByRole("button", { name: "Share", exact: true }).click();
        // #763: shared at once, Riley's key pinned on first use; nothing to
        // compare.
        await expect(p.getByText(/they're asked first/)).toBeVisible({ timeout: 15_000 });
        const said = await p.getByText(/riley/).allInnerTexts();
        if (!said.some((t) => /see|accept|asked|wait/i.test(t))) j.issue("the dialog doesn't say what Riley sees next, or that Riley must accept first");
        await p.keyboard.press("Escape");
        const dialog = p.getByRole("button", { name: "Done" });
        if (await dialog.isVisible()) {
          j.issue("Escape doesn't close the Share dialog; while it's open it covers what comes next (Riley's ask to drive)");
          await dialog.click();
        }
      },
    );

    await step(
      "accept",
      "Riley accepts the shared session",
      "riley",
      "app",
      at(riley),
      (p) => p.getByRole("heading", { name: "A shared session" }),
      async (p) => {
        await p.getByRole("button", { name: "Accept" }).click();
        await expect(p.getByRole("heading", { name: "A shared session" })).toBeHidden();
      },
    );

    await step(
      "find-machine",
      "Riley finds Sam's machine",
      "riley",
      "app",
      at(riley),
      (p) => p.getByText(/sammac/),
      async (p) => {
        await p.getByRole("button", { name: "Switch to sammac" }).click();
        await expect(p.getByTitle("Hosts")).toContainText("sammac", { timeout: 20_000 });
        await expect.poll(() => screenText(p), { timeout: 20_000 }).toContain("hello-42");
      },
    );

    await step(
      "try-typing",
      "Riley clicks into Sam's terminal and types",
      "riley",
      "app",
      at(riley),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        pane = String(await p.evaluate(() => window.__arugula.client.active()));
        await clickTerminal(p);
        await p.keyboard.type("echo first-try\n", { delay: 5 });
        await new Promise((r) => setTimeout(r, 1500));
        const showed = (await screenText(sam.window)).includes("first-try");
        const said = (await p.locator("body").innerText()).match(/[^\n]*(can't type|ask|drive|allow)[^\n]*/i)?.[0];
        if (!showed)
          j.issue(said ? `typing did nothing; the screen said: "${said.trim()}"` : "typing did nothing, and nothing on screen said why or what to do");
      },
    );

    await step(
      "ask-to-drive",
      "Riley asks Sam to let them drive",
      "riley",
      "app",
      at(riley),
      (p) => p.getByText(/trust you with it|Ask the owner to let me drive/i),
      async (p) => {
        const v = p.viewportSize()!;
        await p.mouse.click(v.width / 2, v.height / 2, { button: "right" });
        await p.getByRole("menuitem", { name: "Ask the owner to let me drive it" }).click();
      },
    );

    await step(
      "allow",
      "Sam allows Riley to drive, for 10 minutes",
      "sam",
      "app",
      at(sam),
      (p) => p.getByText(/riley asks to drive/),
      async (p) => {
        const ask = p.getByText(/riley asks to drive/).locator("..");
        await ask.getByRole("combobox").selectOption("10");
        await ask.getByRole("button", { name: "Allow" }).click();
      },
    );

    await step(
      "riley-types",
      "Riley, allowed now, types in Sam's terminal",
      "riley",
      "app",
      at(riley),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await clickTerminal(p);
        await p.keyboard.type("echo second-try\n", { delay: 5 });
        await new Promise((r) => setTimeout(r, 1500));
        if (!(await screenText(sam.window)).includes("second-try")) {
          const said = (await p.locator("body").innerText()).match(/[^\n]*(control|driv)[^\n]*/i)?.[0];
          j.issue(
            said
              ? `allowed, typing still did nothing: Sam still has control of the pane; the screen said: "${said.trim()}"`
              : "allowed, typing still did nothing: Sam still has control of the pane, and nothing on screen says so but a ✎ sam badge",
          );
        }
      },
    );

    await step(
      "take-control",
      "Riley takes control of the pane",
      "riley",
      "app",
      at(riley),
      (p) => p.getByText(/take control|ask for control/i),
      async (p) => {
        // On the pane, beside whose control it's in.
        await p.getByRole("button", { name: "Take control" }).first().click();
      },
    );

    await step(
      "riley-drives",
      "Riley types in Sam's terminal",
      "riley",
      "app",
      at(riley),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await clickTerminal(p);
        j.typed(`echo ${marker}`);
        await p.keyboard.type(`echo ${marker}\n`, { delay: 5 });
        await expect.poll(() => screenText(p), { timeout: 15_000 }).toContain("hi-from-riley-42");
      },
    );

    await step(
      "sam-sees",
      "Sam sees what Riley typed, in his own terminal",
      "sam",
      "app",
      at(sam),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await expect.poll(() => screenText(p), { timeout: 15_000 }).toContain("hi-from-riley-42");
        void pane;
      },
    );
  } catch (e) {
    await j.attach();
    throw e;
  } finally {
    await sam.quit().catch(() => {});
    await riley.quit().catch(() => {});
  }
  await j.finish();
});
