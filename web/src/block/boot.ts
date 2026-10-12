// The bootstrap page's script (M50; S27's web/boot.ts): what control serves
// for any navigation on a block's origin that the worker didn't answer (the
// first load, a hard reload, a cleared worker). It registers the worker,
// gets the block's credentials from the parent page, gives them to the
// worker and loads the page again, now through the worker.
//
// Built by itself into dist/block/boot.js (vite.block.config.ts) and served
// by control at /.arugula/boot.js.

import { save } from "./channel.ts";
import { askParent, controlOrigins, giveWorker } from "./page.ts";

const state = document.getElementById("arugula-block-state");
const say = (t: string) => {
  if (state) state.textContent = t;
};

async function boot() {
  // A page that keeps coming back here isn't getting through the worker.
  const now = Date.now();
  let recent: number[] = [];
  try {
    recent = (JSON.parse(sessionStorage.getItem("arugula-block-boots") ?? "[]") as number[]).filter((t) => now - t < 10_000);
    recent.push(now);
    sessionStorage.setItem("arugula-block-boots", JSON.stringify(recent));
  } catch {}
  if (recent.length > 4) throw new Error("the block's worker isn't answering its pages");
  if (!("serviceWorker" in navigator)) throw new Error("this browser has no service workers here");

  const reg = await navigator.serviceWorker.register("/.arugula/sw.js", { scope: "/" });
  const held = await askParent(controlOrigins(document), "hello");
  await save("held", held);
  await new Promise<void>((res) => {
    const w = reg.active;
    if (w?.state === "activated") return res();
    const sw = (reg.installing ?? reg.waiting ?? w)!;
    sw.addEventListener("statechange", () => sw.state === "activated" && res());
  });
  await giveWorker(held);
  say("Loading…");
  location.replace(location.href);
}

boot().catch((e: unknown) => say(`This block can't load: ${e instanceof Error ? e.message : String(e)}`));
