// J2b (#551): two friends link up through a team. Riley makes DLex Corp
// and sends Sam a one-click invite; Sam joins, puts his own machine in the
// team (as the Teams panel says: In … in Devices and machines…), and Riley,
// who drives in the team, types in Sam's terminal with no ask, as a team
// machine allows. Each person's first run is its own report
// (J2b-setup-<login>). Rules as in journey-j1.spec.ts.
//   just journey j2b

import { expect, test, type Locator, type Page } from "@playwright/test";
import type { App } from "./journey/app";
import { clickTerminal, onboarded, screenText } from "./journey/first-run";
import { Journey, type Surface } from "./journey/record";
import { account, join, makeTeam, moveInto, show } from "./journey/ui";
import { World } from "./journey/world";
import { closeContexts } from "./helpers";

test.afterAll(closeContexts);
test.use({ actionTimeout: 20_000 });

const world = new World();
test.beforeAll(() => world.start());
test.afterAll(() => world.stop());

test("J2b: two friends link up through a team; the friend types on the owner's team machine", async ({ browser }, info) => {
  test.setTimeout(420_000);
  const sam = await onboarded(world, browser, info, "J2b", "sam", "sammac");
  const riley = await onboarded(world, browser, info, "J2b", "riley", "rileymac");
  const j = new Journey("J2b", "two friends link up through a team: invite, the machine in, the friend drives", info, ["sam", "riley"]);
  const at = (who: App) => () => who.window;
  const step = (
    id: string,
    title: string,
    actor: "sam" | "riley",
    surface: Surface,
    page: () => Page,
    prompt: (p: Page) => Locator | null,
    act: (p: Page) => Promise<void>,
    wanted?: string,
  ) => j.step({ id, title, actor, surface, page, prompt: prompt(page()), expect: wanted }, () => act(page()));
  let invite = "";
  const marker = "hi-from-riley-$((6*7))";

  try {
    await step(
      "make-team",
      "Riley makes the team DLex Corp and an invite link",
      "riley",
      "app",
      at(riley),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        invite = await makeTeam(p, "DLex Corp");
      },
    );

    await j.step(
      { id: "send-invite", title: "Riley sends Sam the invite link", actor: "riley", surface: "message", prompt: null, own: "a message to Sam with the link" },
      async () => {},
    );

    await j.step(
      { id: "sam-joins", title: "Sam joins DLex Corp from the link", actor: "sam", surface: "browser", prompt: null, own: "Sam opens the link Riley sent" },
      async () => {
        const tab = await join(sam, invite, "DLex Corp");
        const said = await tab
          .getByText(/You're in DLex Corp/)
          .locator("..")
          .innerText();
        if (!/machine|In …|join/i.test(said)) j.issue("joining doesn't say how to bring a machine into the team");
        await tab.close();
      },
    );

    await step(
      "how-to-add",
      "Sam reads how to add his machine to the team",
      "sam",
      "app",
      at(sam),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await account(p, "Teams…");
        const how = p.getByText(/Add a machine to it with/).first();
        if (
          !(await how.waitFor({ timeout: 5_000 }).then(
            () => true,
            () => false,
          ))
        )
          j.issue("the Teams panel doesn't say how a member adds a machine");
        await p.getByRole("button", { name: "Done" }).click();
      },
    );

    await step(
      "move-in",
      "Sam puts sammac in DLex Corp with In …",
      "sam",
      "app",
      at(sam),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await moveInto(p, "sammac", "DLex Corp");
      },
    );

    await step(
      "riley-finds",
      "Riley finds Sam's machine",
      "riley",
      "app",
      at(riley),
      (p) => p.getByText(/sammac/),
      async (p) => {
        await show(p, "sammac");
        await expect.poll(() => screenText(p), { timeout: 20_000 }).toContain("hello-42");
      },
      "anything naming Sam's machine (sammac) outside the host menu",
    );

    await step(
      "riley-types",
      "Riley types in Sam's terminal, as a member who drives",
      "riley",
      "app",
      at(riley),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await clickTerminal(p);
        j.typed(`echo ${marker}`);
        await p.keyboard.type(`echo ${marker}\n`, { delay: 5 });
        await new Promise((r) => setTimeout(r, 2000));
        if (!(await screenText(p)).includes("hi-from-riley-42")) {
          const said = (await p.locator("body").innerText()).match(/[^\n]*(trust you|can't type|watching|control)[^\n]*/i)?.[0];
          throw new Error(
            said ? `typing did nothing on a team machine; it said: "${said.trim()}"` : "typing did nothing on a team machine, and nothing said why",
          );
        }
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
        await show(p, "sammac");
        await expect.poll(() => screenText(p), { timeout: 15_000 }).toContain("hi-from-riley-42");
      },
    );
  } catch (e) {
    await j.attach();
    throw e;
  } finally {
    for (const a of [sam, riley]) await a.quit().catch(() => {});
  }
  await j.finish({
    // Filed on #551.
    "riley-finds": "#551: a team's machine is named only in the host menu",
  });
});
