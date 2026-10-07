// J2c (#551): the evening #551 audits, replayed. Sam's Mac is already in
// a team (Arugula, with Jake and his machine geek). Riley makes a second
// team, DLex Corp, to work with Sam, and invites him. Sam tries what the
// screens offer to bring his machine into it: the team page's copied
// `arugulad join … --team` (not on his PATH from the app, #550; and the
// machine is already in Arugula), then *In …* (which would take it out of
// Arugula). What works is sharing the session with DLex Corp; Riley finds
// it, the Swarm, and drives Sam's terminal. Sam, looking at Jake's geek,
// opens a new tab with + and it lands there.
//
// Everyone's first run is its own report (J2c-setup-<login>), and Arugula
// is set up through the UI as J2c-arugula, so nothing before the evening
// can hide what happened in it. Rules as in journey-j1.spec.ts.
//   just journey j2c

import { expect, test, type Locator, type Page } from "@playwright/test";
import type { App } from "./journey/app";
import { clickTerminal, onboarded, screenText } from "./journey/first-run";
import { account, join, makeTeam, moveInto, show } from "./journey/ui";
import { Journey, type Surface } from "./journey/record";
import { BIN, World } from "./journey/world";
import { closeContexts } from "./helpers";

test.afterAll(closeContexts);
test.use({ actionTimeout: 20_000 });

const world = new World();
test.beforeAll(() => world.start());
test.afterAll(() => world.stop());

test("J2c: #551's evening: a machine already in one team, a friend's second team, and what gets them linked", async ({ browser }, info) => {
  test.setTimeout(600_000);
  const sam = await onboarded(world, browser, info, "J2c", "sam", "sammac");
  const jake = await onboarded(world, browser, info, "J2c", "jake", "geek");
  const riley = await onboarded(world, browser, info, "J2c", "riley", "rileymac");
  const at = (who: App) => () => who.window;

  // Before the evening: Sam's Arugula, with Jake, and both machines in it.
  const setup = new Journey("J2c-arugula", "before the evening: team Arugula with Sam's and Jake's machines in it", info, ["sam", "jake"]);
  try {
    let link = "";
    await setup.step(
      { id: "make-arugula", title: "Sam makes the team Arugula", actor: "sam", surface: "app", page: at(sam), prompt: null, own: "Sam makes a team for work" },
      async () => {
        link = await makeTeam(sam.window, "Arugula");
      },
    );
    await setup.step(
      { id: "jake-joins", title: "Jake joins Arugula from Sam's link", actor: "jake", surface: "browser", prompt: null, own: "Jake opens the link Sam sent" },
      async () => {
        await join(jake, link, "Arugula");
      },
    );
    await setup.step(
      { id: "geek-in", title: "Jake puts geek in Arugula", actor: "jake", surface: "app", page: at(jake), prompt: null, own: "Jake moves his machine in" },
      async () => {
        await moveInto(jake.window, "geek", "Arugula");
      },
    );
    await setup.step(
      { id: "sammac-in", title: "Sam puts sammac in Arugula", actor: "sam", surface: "app", page: at(sam), prompt: null, own: "Sam moves his machine in" },
      async () => {
        await moveInto(sam.window, "sammac", "Arugula");
        await show(sam.window, "geek");
      },
    );
  } finally {
    await setup.attach();
  }

  const j = new Journey("J2c", "#551's evening: already in a team, a friend's second team, linked by sharing", info, ["sam", "riley"]);
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
  let joinCmd = "";
  const marker = "hi-from-riley-$((6*7))";

  try {
    await step(
      "dlex",
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
        await tab.close();
      },
    );

    await step(
      "team-page",
      "Sam looks for how to bring his machine into DLex Corp",
      "sam",
      "app",
      at(sam),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await account(p, "Teams…");
        joinCmd =
          /arugulad join \S+ --team \S+/.exec(
            await p
              .getByText(/arugulad join .* --team/)
              .last()
              .innerText(),
          )?.[0] ?? "";
        expect(joinCmd).not.toBe("");
        await p.getByRole("button", { name: "Done" }).click();
      },
    );

    await step(
      "copied-join",
      "Sam runs the join command the team page shows",
      "sam",
      "terminal",
      at(sam),
      (p) => p.getByTitle("Hosts"),
      async () => {
        j.typed(joinCmd);
        const r = sam.person.shell(joinCmd);
        if (r.code !== 0) j.issue(`the copied command fails: "${r.out.split("\n")[0]}"`);
      },
    );

    let fullPath = "";
    await j.step(
      {
        id: "find-arugulad",
        title: "Sam finds arugulad inside the app",
        actor: "sam",
        surface: "terminal",
        prompt: null,
        why: "nothing on screen says the app's arugulad is inside Arugula.app (#550)",
      },
      async () => {
        // /Applications/Arugula.app/Contents/MacOS/arugulad, on a Mac.
        fullPath = joinCmd.replace(/^arugulad/, `${BIN}/arugulad`);
      },
    );

    await step(
      "already-in",
      "Sam runs it from the app: the machine is already in Arugula",
      "sam",
      "terminal",
      at(sam),
      (p) => p.getByTitle("Hosts"),
      async () => {
        j.typed(fullPath.replace(BIN, "/Applications/Arugula.app/Contents/MacOS"));
        const r = sam.person.shell(`${fullPath} </dev/null`);
        const said = r.out.split("\n").find((l) => /already|leave|team/i.test(l)) ?? r.out.split("\n")[0];
        if (r.code !== 0) {
          j.issue(`it says: "${said}"`);
          if (!/DLex Corp/.test(r.out)) j.issue("it names the team the machine is in, but not DLex Corp, the one asked for");
          if (/arugulad leave/.test(r.out))
            j.issue(
              "its advice (arugulad leave, then join) takes the machine off the cloud until a fresh approval; In … in Devices and machines… would move it",
            );
          if (!/share/i.test(r.out)) j.issue("it doesn't say a machine is in one place, or offer sharing a session with the other team instead");
        }
      },
    );

    await step(
      "try-move",
      "Sam tries In … in Devices and machines…",
      "sam",
      "app",
      at(sam),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await account(p, "Devices and machines…");
        const row = p
          .getByRole("listitem")
          .filter({ hasText: "sammac" })
          .filter({ has: p.getByRole("combobox") });
        await row.getByRole("combobox").selectOption({ label: "DLex Corp" });
        const explain = await row.getByText(/sees sammac|lose/).innerText();
        if (/lose/.test(explain)) j.issue(`moving says: "${explain.split(".")[0]}." One machine is in one team at a time`);
        if (!/share/i.test(explain)) j.issue("it doesn't offer the alternative: keep it in Arugula and share a session with DLex Corp");
        await row.getByRole("button", { name: "Cancel" }).click();
        await p.getByRole("button", { name: "Done" }).click();
      },
    );

    await step(
      "share-team",
      "Sam shares his session with everyone in DLex Corp",
      "sam",
      "app",
      at(sam),
      (p) => p.getByRole("button", { name: /share/i }),
      async (p) => {
        await show(p, "sammac");
        await p.getByTitle("Sessions").click();
        await p.getByRole("menuitem", { name: "Share session…" }).click();
        await p.getByRole("button", { name: "Share with everyone in DLex Corp" }).click();
        await expect(p.getByText("DLex Corp").first()).toBeVisible();
        const row = p.getByRole("listitem").filter({ hasText: "DLex Corp" }).first();
        const role = await row
          .getByRole("combobox")
          .inputValue()
          .catch(() => "");
        if (role === "viewer")
          j.issue(
            "sharing with the team gives it 'watches': the role picker is down in the form for adding one person, not beside Share with everyone in DLex Corp",
          );
        const dialog = await p
          .getByText(/People you share with/)
          .locator("..")
          .innerText();
        if (!/host menu|swarm|where/i.test(dialog)) j.issue("the dialog doesn't say where DLex Corp's members see the session");
        if (!/accept|aren't asked|not asked/i.test(dialog)) j.issue("it doesn't say whether teammates must accept first (they aren't asked)");
        if (/can't type on this machine unless you trust/i.test(dialog))
          j.issue("'drives' here still means asking Sam per pane, on his own machine; that's easy to miss");
        await p.getByRole("button", { name: "Done" }).click();
      },
      "a Share button on the main screen",
    );

    await step(
      "riley-finds",
      "Riley finds Sam's shared session",
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
      "swarm",
      "Riley opens the Swarm to see what Sam's working on",
      "riley",
      "app",
      at(riley),
      (p) => p.getByText("Swarm", { exact: true }),
      async (p) => {
        await p.getByText("Swarm", { exact: true }).click();
        await new Promise((r) => setTimeout(r, 1500));
        const text = await p.locator("body").innerText();
        const empty = text.match(/[^\n]*no projects?[^\n]*/i)?.[0];
        const clusters = text.match(/^\/\S+$/m)?.[0];
        if (empty) j.issue(`the Swarm says "${empty.trim()}"`);
        else if (clusters) j.issue(`by project, Sam's terminal shows as "${clusters}", its directory`);
        if (!/git|repositor/i.test(text)) j.issue("nothing says projects come from a pane's git repository (a terminal in $HOME has none), or how to make one");
        // Back to the panes: the Swarm covers the bar, and has no close.
        const back = p.getByText("Panes", { exact: true });
        if (
          !(await back.click({ timeout: 2_000 }).then(
            () => true,
            () => false,
          ))
        ) {
          await p.keyboard.press("Escape");
          if (new URL(p.url()).hash === "#swarm") {
            j.issue(
              "nothing on the Swarm leads back to the panes, and Escape doesn't; the app's window has no Back (the journey went back through history, as a browser's Back would)",
            );
            await p.goBack();
          }
        }
        await expect(p.getByTitle("Hosts")).toBeVisible();
      },
    );

    let watching = false;
    await step(
      "riley-tries",
      "Riley tries to type in Sam's terminal",
      "riley",
      "app",
      at(riley),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await clickTerminal(p);
        await p.keyboard.type("echo first-try\n", { delay: 5 });
        await new Promise((r) => setTimeout(r, 1500));
        if (!(await screenText(sam.window)).includes("first-try")) {
          const said = (await p.locator("body").innerText()).match(/[^\n]*(trust you|can't type|watching)[^\n]*/i)?.[0];
          watching = /watching/i.test(said ?? "");
          j.issue(said ? `typing did nothing; it said: "${said.trim()}"` : "typing did nothing, and nothing said why");
        }
      },
    );

    if (watching) {
      await j.step(
        {
          id: "only-watch",
          title: "Riley tells Sam they can only watch",
          actor: "riley",
          surface: "message",
          prompt: null,
          own: "a message to Sam: it says I'm watching",
        },
        async () => {},
      );
      await step(
        "make-drive",
        "Sam lets DLex Corp drive, in Share session…",
        "sam",
        "app",
        at(sam),
        (p) => p.getByTitle("Sessions"),
        async (p) => {
          await show(p, "sammac");
          await p.getByTitle("Sessions").click();
          await p.getByRole("menuitem", { name: "Share session…" }).click();
          const row = p.getByRole("listitem").filter({ hasText: "DLex Corp" }).first();
          await row.getByRole("combobox").selectOption({ label: "drives" });
          await p.getByRole("button", { name: "Done" }).click();
        },
      );
    }

    if (watching)
      await step(
        "riley-tries-again",
        "Riley, now a driver, tries typing again",
        "riley",
        "app",
        at(riley),
        (p) => p.getByTitle("Hosts"),
        async (p) => {
          await clickTerminal(p);
          await p.keyboard.type("echo second-try\n", { delay: 5 });
          await new Promise((r) => setTimeout(r, 1500));
          if (!(await screenText(sam.window)).includes("second-try")) {
            const said = (await p.locator("body").innerText()).match(/[^\n]*(trust you|can't type|watching)[^\n]*/i)?.[0];
            j.issue(said ? `typing still did nothing; it said: "${said.trim()}"` : "typing still did nothing, and nothing said why");
          }
        },
      );

    await step(
      "riley-asks",
      "Riley asks Sam to let them drive",
      "riley",
      "app",
      at(riley),
      (p) => p.getByText(/trust you with it|Ask the owner to let me drive/i),
      async (p) => {
        const v = p.viewportSize()!;
        await p.mouse.click(v.width / 2, v.height / 2, { button: "right" });
        const ask = p.getByRole("menuitem", { name: "Ask the owner to let me drive it" });
        if (!(await ask.isVisible({ timeout: 3_000 }).catch(() => false))) {
          const items = await p.getByRole("menuitem").allInnerTexts();
          throw new Error(`no "Ask the owner to let me drive it" in the pane menu; it has: ${items.map((t) => t.trim()).join(" | ")}`);
        }
        await ask.click();
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
      "take-control",
      "Riley takes control of the pane",
      "riley",
      "app",
      at(riley),
      (p) => p.getByText(/take control|ask for control/i),
      async (p) => {
        const v = p.viewportSize()!;
        await p.mouse.click(v.width / 2, v.height / 2, { button: "right" });
        await p.getByRole("menuitem", { name: /Take control/ }).click();
      },
    );

    await step(
      "riley-drives",
      "Riley types in Sam's terminal, and Sam sees it",
      "riley",
      "app",
      at(riley),
      (p) => p.getByTitle("Hosts"),
      async (p) => {
        await clickTerminal(p);
        j.typed(`echo ${marker}`);
        await p.keyboard.type(`echo ${marker}\n`, { delay: 5 });
        await expect.poll(() => screenText(p), { timeout: 15_000 }).toContain("hi-from-riley-42");
        await expect.poll(() => screenText(sam.window), { timeout: 15_000 }).toContain("hi-from-riley-42");
      },
    );

    await step(
      "plus-on-geek",
      "Sam, looking at Jake's geek, opens a new tab with +",
      "sam",
      "app",
      at(sam),
      (p) => p.getByRole("button", { name: "+" }),
      async (p) => {
        await show(p, "geek");
        const title = (await p.getByRole("button", { name: "+" }).getAttribute("title")) ?? "";
        if (!/geek|machine|on /i.test(title))
          j.issue(`+ doesn't say which machine the tab opens on (its tooltip: "${title}"); this one opens on Jake's geek, as jake`);
        await p.getByRole("button", { name: "+" }).click();
        await expect(p.getByTitle("Hosts")).toContainText("geek");
      },
    );
  } catch (e) {
    await j.attach();
    throw e;
  } finally {
    for (const a of [sam, jake, riley]) await a.quit().catch(() => {});
  }
  await j.finish({
    // Filed on #551 and #550.
    "find-arugulad": "#550: the app's arugulad isn't on PATH, and nothing says where it is",
    "share-team": "#551: Share is only in the session menu",
    "riley-finds": "#551: a shared machine is named only in the host menu",
    "riley-asks": "#551: after the role change the screen still says you're watching; nothing says to ask",
    "take-control": "#551: allowed to drive, but the owner has control, and nothing says Take control",
  });
});
