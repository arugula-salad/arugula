// Ctrl+Enter in a pane sends CSI 13;5u while the program has the kitty
// keyboard protocol on (CSI > u), and CR while it's off (#412); xterm.js
// alone always sends CR. A program in raw mode logs each byte it reads and
// turns the protocol on when it reads "k", off again on "p", so the test
// sees exactly what reached the pane under each.

import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { active, paneEl, ready, reset, run, text } from "./helpers";

test("Ctrl+Enter sends CSI 13;5u with the kitty protocol on, CR with it off", async ({ page }) => {
  await reset(page);
  const pane = await active(page);
  const dir = mkdtempSync(join(tmpdir(), "ctrl-enter-"));
  const log = join(dir, "bytes");
  const script = join(dir, "raw.py");
  writeFileSync(
    script,
    [
      "import os, sys, tty",
      "tty.setraw(0)",
      "print('raw-ready\\r', flush=True)",
      "while True:",
      "    b = os.read(0, 1)",
      "    with open(sys.argv[1], 'a') as f:",
      "        f.write('%02x\\n' % b[0])",
      "    if b == b'k':",
      "        sys.stdout.write('\\x1b[>1ukitty-on\\r\\n'); sys.stdout.flush()",
      "    if b == b'p':",
      "        sys.stdout.write('\\x1b[<ukitty-off\\r\\n'); sys.stdout.flush()",
      "    if b == b'q':",
      "        break",
      "",
    ].join("\n"),
  );
  writeFileSync(log, "");
  await run(page, pane, `python3 ${script} ${log}`, "raw-ready");
  await ready(page, pane);
  const bytes = () => readFileSync(log, "utf8").trim().split("\n").filter(Boolean);
  const seen: string[] = [];
  const expectBytes = async (...more: string[]) => {
    seen.push(...more);
    await expect.poll(bytes).toEqual(seen);
  };

  await paneEl(page, pane).click({ position: { x: 40, y: 40 } });
  // Off: a plain CR, nothing else.
  await page.keyboard.press("Control+Enter");
  await page.keyboard.press("x");
  await expectBytes("0d", "78");

  // On: the screen shows "kitty-on" only after the push before it is parsed.
  await page.keyboard.press("k");
  await expectBytes("6b");
  await expect.poll(() => text(page, pane)).toContain("kitty-on");
  await page.keyboard.press("Control+Enter");
  await page.keyboard.press("x");
  await expectBytes(..."\x1b[13;5u".split("").map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")), "78");
  // Enter alone is still CR under the protocol's first flag.
  await page.keyboard.press("Enter");
  await expectBytes("0d");

  // Off again after the pop.
  await page.keyboard.press("p");
  await expectBytes("70");
  await expect.poll(() => text(page, pane)).toContain("kitty-off");
  await page.keyboard.press("Control+Enter");
  await page.keyboard.press("x");
  await expectBytes("0d", "78");

  await page.keyboard.press("q");
  await expectBytes("71");
});
