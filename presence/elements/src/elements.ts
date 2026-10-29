// The custom elements (../../sdk/API.md): <arugula-room> joins a room (a
// token from your endpoint, or the relay's demo tokens to prototype) and
// re-dispatches its events; <arugula-camera>, <arugula-people> and
// <arugula-avatar> inside it draw from it, each in a shadow root styled
// from --arugula-* custom properties.

import { connect, httpOrigin, RELAY, type Room } from "./room.ts";
import { mountPeople, mountAvatar } from "./view.ts";
import { mountCamera } from "./controls.ts";
import { CSS } from "./styles.ts";

type Look = { character: number; colour: number };

export class ArugulaRoom extends HTMLElement {
  private room: Room | null = null;
  private waiting: ((r: Room) => void)[] = [];
  private failed: Error | null = null;

  /** The room once joined (children wait on it). */
  whenReady(): Promise<Room> {
    if (this.room) return Promise.resolve(this.room);
    if (this.failed) return Promise.reject(this.failed);
    return new Promise((ok) => this.waiting.push(ok));
  }

  connectedCallback() {
    if (this.room) return;
    const room = this.getAttribute("room");
    if (!room) return console.error("<arugula-room> needs a room attribute");
    const relay = this.getAttribute("relay") || RELAY;
    const tokenUrl = this.getAttribute("token-url");
    const demo = this.hasAttribute("demo");
    if (!tokenUrl && !demo) return console.error("<arugula-room> needs token-url, or demo to prototype");
    const token = async () => {
      const r = demo
        ? await fetch(`${httpOrigin(relay)}/api/demo/token`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ room, name: (this.getAttribute("name") || "Guest").slice(0, 80) }),
          })
        : await fetch(tokenUrl!, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ room }), credentials: "include" });
      if (!r.ok) throw new Error(`the token endpoint said ${r.status}`);
      return ((await r.json()) as { token: string }).token;
    };
    const num = (a: string) => (this.hasAttribute(a) ? Number(this.getAttribute(a)) : undefined);
    const character = num("character"), colour = num("colour");
    const look: Look | undefined = character !== undefined || colour !== undefined ? { character: character ?? 2, colour: colour ?? 0 } : undefined;
    const mode = this.getAttribute("mode") === "outline" ? "outline" : "avatar";
    void connect({ room, token, relay, mode, look })
      .then((r) => {
        this.room = r;
        const fire = (type: string, detail: unknown) => this.dispatchEvent(new CustomEvent(`arugula:${type}`, { detail, bubbles: true, composed: true }));
        r.on("members", (members) => fire("members", { members }));
        r.on("knock", (from) => fire("knock", { from }));
        r.on("answer", (from, yes) => fire("answer", { from, yes }));
        r.on("live", (state) => fire("live", { state }));
        fire("members", { members: r.members });
        for (const ok of this.waiting.splice(0)) ok(r);
      })
      .catch((e: Error) => {
        this.failed = e;
        console.error(`<arugula-room>: ${e.message}`);
      });
  }

  disconnectedCallback() {
    this.room?.leave();
    this.room = null;
  }
}

/** An element drawn from the nearest <arugula-room> around it. */
abstract class InRoom extends HTMLElement {
  protected root: ShadowRoot;
  private stop: (() => void) | null = null;
  constructor() {
    super();
    this.root = this.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = CSS;
    this.root.append(style);
  }

  protected abstract draw(room: Room): () => void;

  async connectedCallback() {
    await customElements.whenDefined("arugula-room");
    const host = this.closest("arugula-room") as ArugulaRoom | null;
    if (!host) return console.error(`<${this.localName}> goes inside an <arugula-room>`);
    const room = await host.whenReady().catch(() => null);
    if (!room || !this.isConnected) return;
    this.stop?.();
    this.stop = this.draw(room);
  }

  disconnectedCallback() {
    this.stop?.();
    this.stop = null;
  }
}

export class ArugulaPeople extends InRoom {
  protected draw(room: Room) {
    return mountPeople(room, this.root, {
      size: this.getAttribute("size") === "head" ? "head" : "bust",
      self: this.getAttribute("self") === "hide" ? "hide" : "first",
    });
  }
}

export class ArugulaCamera extends InRoom {
  protected draw(room: Room) {
    return mountCamera(room, this.root, { controls: this.getAttribute("controls") === "button" ? "button" : "full" });
  }
}

export class ArugulaAvatar extends InRoom {
  static observedAttributes = ["user", "size"];
  protected draw(room: Room) {
    return mountAvatar(room, this.root, this.getAttribute("user") ?? "", { size: this.getAttribute("size") === "bust" ? "bust" : "head" });
  }
  attributeChangedCallback() {
    if (this.isConnected) void this.connectedCallback();
  }
}

/** Define the elements (once; safe to call again). */
export function defineElements() {
  if (typeof customElements === "undefined") return;
  const all: [string, CustomElementConstructor][] = [
    ["arugula-room", ArugulaRoom],
    ["arugula-people", ArugulaPeople],
    ["arugula-camera", ArugulaCamera],
    ["arugula-avatar", ArugulaAvatar],
  ];
  for (const [name, el] of all) if (!customElements.get(name)) customElements.define(name, el);
}
