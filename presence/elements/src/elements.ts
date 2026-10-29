// The custom elements (../../sdk/API.md): <arugula-room> joins a room (a
// token from your endpoint; or a publishable key with the person's own
// sign-in token, or as a guest; or the relay's demo tokens to prototype) and
// re-dispatches its events; <arugula-camera>, <arugula-people>,
// <arugula-avatar> and <arugula-chat> inside it draw from it, each in a shadow root styled
// from --arugula-* custom properties.

import { connect, httpOrigin, RELAY, type ConnectOptions, type Room } from "./room.ts";
import { mountPeople, mountAvatar } from "./view.ts";
import { mountCamera } from "./controls.ts";
import { mountChat } from "./chat.ts";
import { CSS } from "./styles.ts";

type Look = { character: number; colour: number };

export class ArugulaRoom extends HTMLElement {
  private room: Room | null = null;
  private waiting: ((r: Room) => void)[] = [];
  private failed: Error | null = null;
  private joining = false;
  private authValue: ConnectOptions["auth"] | null = null;

  /** With `key`: the person's token from your auth provider, or a function
   * for a fresh one (called on every (re)connect). Set it before or after
   * the element is in the page; `<arugula-room key auth>` waits for it. */
  get auth() {
    return this.authValue;
  }
  set auth(v) {
    this.authValue = v;
    if (v && this.isConnected) this.connectedCallback();
  }

  /** The room once joined (children wait on it). */
  whenReady(): Promise<Room> {
    if (this.room) return Promise.resolve(this.room);
    if (this.failed) return Promise.reject(this.failed);
    return new Promise((ok) => this.waiting.push(ok));
  }

  connectedCallback() {
    // `el.auth = …` before this element was defined set a plain property:
    // through the setter instead.
    if (Object.prototype.hasOwnProperty.call(this, "auth")) {
      const v = (this as { auth?: ConnectOptions["auth"] }).auth;
      delete (this as { auth?: unknown }).auth;
      this.authValue = v ?? null;
    }
    if (this.room || this.joining) return;
    const room = this.getAttribute("room");
    if (!room) return console.error("<arugula-room> needs a room attribute");
    const relay = this.getAttribute("relay") || RELAY;
    const tokenUrl = this.getAttribute("token-url");
    const demo = this.hasAttribute("demo");
    const key = this.getAttribute("key");
    const auth = this.getAttribute("auth") || this.authValue;
    if (key && !auth && this.hasAttribute("auth")) return; // waits for el.auth
    if (!tokenUrl && !demo && !key) return console.error("<arugula-room> needs token-url, or key, or demo to prototype");
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
    const name = this.getAttribute("name") || undefined;
    const how: Partial<ConnectOptions> = key ? (auth ? { key, auth, name } : { key, guest: (name || "Guest").slice(0, 80) }) : { token };
    this.joining = true;
    void connect({ room, relay, mode, look, ...how })
      .then((r) => {
        this.room = r;
        const fire = (type: string, detail: unknown) => this.dispatchEvent(new CustomEvent(`arugula:${type}`, { detail, bubbles: true, composed: true }));
        r.on("members", (members) => fire("members", { members }));
        r.on("wave", (from) => fire("wave", { from }));
        r.on("answer", (from, yes) => fire("answer", { from, yes }));
        r.on("react", (from, emoji) => fire("react", { from, emoji }));
        r.on("message", (message) => fire("message", { message }));
        r.on("live", (state) => fire("live", { state }));
        fire("members", { members: r.members });
        for (const ok of this.waiting.splice(0)) ok(r);
      })
      .catch((e: Error) => {
        this.joining = false;
        this.failed = e;
        console.error(`<arugula-room>: ${e.message}`);
      });
  }

  disconnectedCallback() {
    this.room?.leave();
    this.room = null;
    this.joining = false;
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

export class ArugulaChat extends InRoom {
  protected draw(room: Room) {
    return mountChat(room, this.root, { placeholder: this.getAttribute("placeholder") ?? undefined });
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
    ["arugula-chat", ArugulaChat],
  ];
  for (const [name, el] of all) if (!customElements.get(name)) customElements.define(name, el);
}
