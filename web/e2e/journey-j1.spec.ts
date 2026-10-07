// J1 (#551): one person, from no account to using Arugula, the way a new
// Mac user does: the app's first window, Getting started's Cloud step, an
// account made from the machine's approval link, the machine approved,
// the app signed in and approved, a command run in a terminal, and the
// app restarted. One run, from a clean home and an empty control.
//
// Every step acts only on what the person sees (roles and visible text);
// codes and fingerprints are read off one screen and checked on the other.
// The only stand-ins are GitHub, and the app's native parts (journey/app.ts).
// Each step names the prompt that led to it: one with none is unguided,
// and fails the run once the whole path is drawn.
//
// At the end it draws the path as a graph, every step annotated with what
// went right or wrong (journey/report.ts): web/journey-reports/J1.html and
// J1.svg, the newest run, with each run kept under history/. Local output,
// never committed (web/.gitignore). JOURNEY_REPORT_DIR puts it elsewhere.
//   just journey            (or: cd web && pnpm exec playwright test e2e/journey-j1.spec.ts)

import { test } from "@playwright/test";
import { App } from "./journey/app";
import { firstRun } from "./journey/first-run";
import { Journey } from "./journey/record";
import { World } from "./journey/world";
import { closeContexts } from "./helpers";

test.afterAll(closeContexts);
// A person waits a while, not forever: a wrong guess fails fast.
test.use({ actionTimeout: 20_000 });

const world = new World();
test.beforeAll(() => world.start());
test.afterAll(() => world.stop());

test("J1: a new Mac user, from no account to a command in a terminal", async ({
  browser,
}, info) => {
  test.setTimeout(300_000);
  const me = await world.person(browser, "newcomer");
  await me.installApp("newmac");
  const app = await App.install(me, browser);
  const j = new Journey(
    "J1",
    "one person, unregistered to working (Mac app)",
    info,
    ["newcomer"],
  );
  app.onOutside = (url) => j.opened(url);
  try {
    await firstRun(j, world, app, "newcomer", "newmac");
  } catch (e) {
    await j.attach();
    throw e;
  } finally {
    await app.quit().catch(() => {});
  }
  await j.finish();
});
