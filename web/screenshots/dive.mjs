// The README's dive.gif (arugula.io/img/dive.gif): the Overview's zoom in
// the app, Project → Board → Work and back out, on the same made-up team
// as the site's pictures (shots.spec.ts' "dive", which writes the frames;
// `just screenshots` skips it). It needs the daemon built, as `just
// screenshots` does. From web/:
//
//   node screenshots/dive.mjs /tmp/dive-frames
//   gifski --fps 9 --width 880 --quality 70 -o $SITE_DIR/apps/site/public/img/dive.gif /tmp/dive-frames/f*.png
//
// (gifski: `cargo install gifski`; it keeps the GIF near 1.5 MB.)

import { spawnSync } from "node:child_process";
import { rmSync } from "node:fs";
import { resolve } from "node:path";

const out = resolve(process.argv[2] ?? "dive-frames");
rmSync(out, { recursive: true, force: true });
const run = spawnSync("pnpm", ["exec", "playwright", "test", "-c", "screenshots.config.ts", "screenshots/shots.spec.ts", "-g", "zoom, frame by frame"], {
  stdio: "inherit",
  // The other pictures go nowhere.
  env: { ...process.env, DIVE_OUT: out, SHOTS_OUT: resolve(out, "shots") },
});
process.exit(run.status ?? 1);
