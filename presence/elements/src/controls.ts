// Your camera's controls (<arugula-camera>, and the React CameraControls):
// on/off, then avatar or outline, your character and your colour. Redrawn
// from the room's state whenever it changes; nothing kept here.

import { CHARACTERS } from "@arugula/presence-core/avatar/characters/index";
import { COLOURS } from "@arugula/presence-core/avatar/stage";
import type { Room } from "./room.ts";
import { REACTIONS } from "@arugula/presence/client";

export interface CameraOptions {
  /** Everything (default), or the on/off button alone. */
  controls?: "full" | "button";
}

export function mountCamera(room: Room, el: Element | ShadowRoot, o: CameraOptions = {}): () => void {
  const bar = document.createElement("div");
  bar.className = "arugula-camera";
  bar.setAttribute("part", "camera");
  el.append(bar);
  const button = (label: string, on: boolean, run: () => void, cls = "") => {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    b.className = `${cls}${on ? " on" : ""}`.trim();
    b.setAttribute("aria-pressed", String(on));
    b.addEventListener("click", run);
    return b;
  };
  const render = () => {
    const c = room.client;
    const on = c.cameraOn;
    const parts: HTMLElement[] = [button(on ? "Turn off my camera" : "Turn on my camera", on, () => (on ? c.stopCamera() : void c.startCamera()), on ? "" : "primary")];
    if (o.controls !== "button") {
      const mode = document.createElement("span");
      mode.className = "seg";
      mode.append(...(["avatar", "outline"] as const).map((m) => button(m === "avatar" ? "Avatar" : "Outline", c.mode === m, () => c.setMode(m))));
      parts.push(mode);
      if (c.mode === "avatar") {
        const chars = document.createElement("span");
        chars.className = "seg";
        chars.append(...CHARACTERS.map((ch) => button(ch.name, c.look.character === ch.id, () => c.setLook({ ...c.look, character: ch.id }))));
        const colours = document.createElement("span");
        colours.className = "seg";
        colours.append(
          ...COLOURS.map((v, i) => {
            const b = button("", c.look.colour === i, () => c.setLook({ ...c.look, colour: i }), "swatch");
            // The shadow root's stylesheet defines the theme's variables.
            b.style.background = `var(${v})`;
            b.setAttribute("aria-label", v.slice(2));
            return b;
          }),
        );
        parts.push(chars, colours);
      }
      // Reactions: an emoji over your tile, for the room.
      const reacts = document.createElement("span");
      reacts.className = "seg reacts-bar";
      reacts.append(
        ...REACTIONS.map((e) => {
          const b = button(e, false, () => room.react(e), "emoji");
          b.setAttribute("aria-label", `React ${e}`);
          return b;
        }),
      );
      parts.push(reacts);
      if (c.mine?.error) {
        const err = document.createElement("span");
        err.className = "error";
        err.textContent = c.mine.error;
        parts.push(err);
      }
    }
    bar.replaceChildren(...parts);
  };
  const unsub = room.client.subscribe(render);
  render();
  return () => {
    unsub();
    bar.remove();
  };
}

