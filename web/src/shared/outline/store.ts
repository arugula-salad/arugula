// Who's around as outlines (S43), for every People block on the page: one
// stream from control of the frames of those you share a team with, and
// your own camera, if you turned it on. Control relays frames and keeps
// only each person's latest, in memory (crates/platform/src/chat/outlines.rs).
//
// The stream runs while a block that shows it is open (`watch`), and the
// camera runs until you turn it off or close the page; nothing turns it
// on but you.
//
// S44: a camera sends outlines or, in avatar mode, rigs (../avatar): your
// pose as numbers, which every page draws as the character you picked.
// A rig comes a dozen times a second and avatars read it every animation
// frame, so a rig only tells listeners when someone starts or stops.

import { platformFetch } from "../maintenance";
import { startCamera, type Camera, type FrameStats } from "./camera";
import { decodeFrame, type Frame } from "./frame";
import { RIG, unpackRig, type Look, type Rig } from "../avatar/rig";
import type { AvatarCamera } from "../avatar/cam";

/** A frame older than this is someone whose camera went away. */
const STALE = 12_000;

export interface Seen {
  /** An outline frame, or (avatar mode) a rig and the look to draw it in. */
  frame?: Frame;
  rig?: Rig;
  look?: Look;
  at: number;
  bytes: number;
}

const MODE = "arugula.s44.mode";
const LOOK = "arugula.s44.look";

function kept<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}

function keep(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    // Not kept: this page still has it.
  }
}

export interface Mine {
  frame: Frame | null;
  stats: FrameStats | null;
  /** Avatar mode: your newest rig, and how long tracking took. */
  rig: Rig | null;
  trackMs: number;
  /** Bytes sent in the last few seconds, for kbps. */
  sent: { at: number; bytes: number }[];
  error: string;
}

class OutlineStore {
  private seen = new Map<string, Seen>();
  private listeners = new Set<() => void>();
  private es: EventSource | null = null;
  private watchers = 0;
  private sweep: number | undefined;
  private camera: Camera | null = null;
  private avatarCam: AvatarCamera | null = null;
  private starting = false;
  /** The look each person last sent, so their avatar idles in it after
   * their camera goes off. */
  private looks = new Map<string, Look>();
  /** Outline lines or an avatar (S44); yours, kept in this browser. */
  mode: "outline" | "avatar" = kept(MODE, "outline");
  /** Your character and colour (S44). */
  look: Look = kept(LOOK, { character: 2, colour: 0 });
  /** Rig frames a second. */
  rigFps = 12;
  fps = 4;
  detail = 0.7;
  /** Colour lines by what they trace (./labels). */
  label = true;
  /** Labelling's state, and why it stopped if it did. */
  labelState: "loading" | "on" | "off" = "off";
  labelError = "";
  /** Estimate depth (./depth): far lines fainter, labels checked. */
  depth = true;
  depthState: "loading" | "on" | "off" = "off";
  depthError = "";
  mine: Mine | null = null;
  /** The stream's state, for the block to say. */
  live: "off" | "connecting" | "on" | "refused" = "off";

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => void this.listeners.delete(fn);
  }

  of(account: string): Seen | null {
    const s = this.seen.get(account);
    return s && Date.now() - s.at < STALE ? s : null;
  }

  /** The look `account` last sent a rig in, this session. */
  lookOf(account: string): Look | null {
    return this.looks.get(account) ?? null;
  }

  setMode(mode: "outline" | "avatar") {
    if (mode === this.mode) return;
    const was = this.cameraOn;
    if (was) this.stopCamera();
    this.mode = mode;
    keep(MODE, mode);
    this.emit();
    if (was) void this.startCamera();
  }

  setLook(look: Look) {
    this.look = look;
    keep(LOOK, look);
    this.avatarCam?.setLook(look);
    this.emit();
  }

  /** A block shows outlines: the stream runs while one does. */
  watch(): () => void {
    if (this.watchers++ === 0) this.open();
    return () => {
      if (--this.watchers === 0) this.close();
    };
  }

  get cameraOn(): boolean {
    return !!this.camera || !!this.avatarCam || this.starting;
  }

  async startCamera() {
    if (this.cameraOn) return;
    this.starting = true;
    this.mine = { frame: null, stats: null, rig: null, trackMs: 0, sent: [], error: "" };
    this.emit();
    if (this.mode === "avatar") return this.startAvatar();
    try {
      this.camera = await startCamera({
        fps: this.fps,
        detail: this.detail,
        label: this.label,
        onLabelState: (state, error) => {
          this.labelState = state;
          this.labelError = error ?? "";
          this.emit();
        },
        depth: this.depth,
        onDepthState: (state, error) => {
          this.depthState = state;
          this.depthError = error ?? "";
          this.emit();
        },
        onFrame: (frame, stats) => {
          if (!this.mine) return;
          this.mine.frame = frame;
          this.mine.stats = stats;
          if (stats.sent) {
            const now = Date.now();
            this.mine.sent = [...this.mine.sent.filter((s) => now - s.at < 5_000), { at: now, bytes: stats.bytes }];
          }
          this.emit();
        },
        send: (bytes) => void this.post(bytes),
        onError: (e) => {
          if (this.mine) this.mine.error = e.message;
          this.emit();
        },
      });
      addEventListener("pagehide", this.leave);
    } catch (e) {
      this.mine = { frame: null, stats: null, rig: null, trackMs: 0, sent: [], error: cameraRefusal(e as Error) };
    } finally {
      this.starting = false;
      this.emit();
    }
  }

  private async startAvatar() {
    let told = 0;
    try {
      const { startAvatarCamera } = await import("../avatar/cam");
      this.avatarCam = await startAvatarCamera({
        fps: this.rigFps,
        look: this.look,
        onRig: (rig, ms) => {
          if (!this.mine) return;
          this.mine.rig = rig;
          this.mine.trackMs = ms;
        },
        send: (bytes) => {
          if (!this.mine) return;
          const now = Date.now();
          this.mine.sent = [...this.mine.sent.filter((s) => now - s.at < 5_000), { at: now, bytes: bytes.length }];
          void this.post(bytes);
          // The stats line, about once a second.
          if (now - told > 1000) {
            told = now;
            this.emit();
          }
        },
        onError: (e) => {
          if (this.mine) this.mine.error = e.message;
          this.emit();
        },
      });
      addEventListener("pagehide", this.leave);
    } catch (e) {
      this.mine = { frame: null, stats: null, rig: null, trackMs: 0, sent: [], error: cameraRefusal(e as Error) };
    } finally {
      this.starting = false;
      this.emit();
    }
  }

  stopCamera() {
    if (!this.camera && !this.avatarCam) return;
    this.camera?.stop();
    this.camera = null;
    this.avatarCam?.stop();
    this.avatarCam = null;
    this.mine = null;
    removeEventListener("pagehide", this.leave);
    void platformFetch("/api/people/outline/off", { method: "POST", credentials: "same-origin" }).catch(() => {});
    this.emit();
  }

  setRate(fps: number) {
    this.fps = fps;
    this.camera?.setRate(fps);
    this.emit();
  }

  setLabel(on: boolean) {
    this.label = on;
    this.camera?.setLabel(on);
    this.emit();
  }

  setDepth(on: boolean) {
    this.depth = on;
    this.camera?.setDepth(on);
    this.emit();
  }

  setDetail(d: number) {
    this.detail = d;
    this.camera?.setDetail(d);
    this.emit();
  }

  /** Kilobits a second sent, over the last few seconds. */
  kbps(): number {
    const s = this.mine?.sent ?? [];
    if (s.length < 2) return 0;
    const span = Math.max(1, (s[s.length - 1].at - s[0].at) / 1000);
    return (s.slice(1).reduce((n, x) => n + x.bytes, 0) * 8) / 1000 / span;
  }

  private emit() {
    for (const fn of [...this.listeners]) fn();
  }

  private leave = () => {
    navigator.sendBeacon?.("/api/people/outline/off");
  };

  private async post(bytes: Uint8Array) {
    try {
      await platformFetch("/api/people/outline", {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: bytes as BodyInit,
        credentials: "same-origin",
      });
    } catch {
      // A dropped frame: the next one says the same.
    }
  }

  private open() {
    if (typeof EventSource === "undefined") return;
    this.live = "connecting";
    const es = new EventSource("/api/people/outlines");
    this.es = es;
    es.addEventListener("open", () => {
      this.live = "on";
      this.emit();
    });
    es.addEventListener("frame", (e) => void this.frame((e as MessageEvent).data));
    es.addEventListener("off", (e) => {
      const { account } = JSON.parse((e as MessageEvent).data) as { account: string };
      this.seen.delete(account);
      this.emit();
    });
    es.addEventListener("error", () => {
      // EventSource tries again by itself; a refusal (chat off) closes it.
      if (es.readyState === EventSource.CLOSED && this.es === es) this.live = "refused";
      else if (this.es === es) this.live = "connecting";
      this.emit();
    });
    // Someone whose frames stopped without an `off` (a closed laptop).
    this.sweep = window.setInterval(() => this.emit(), 3_000);
    this.emit();
  }

  private close() {
    this.es?.close();
    this.es = null;
    clearInterval(this.sweep);
    this.seen.clear();
    this.live = "off";
    this.emit();
  }

  private async frame(data: string) {
    try {
      const m = JSON.parse(data) as { account: string; at: number; data: string };
      const bytes = Uint8Array.from(atob(m.data), (c) => c.charCodeAt(0));
      if (bytes[0] === 0 && bytes[1] === RIG) {
        const { rig, look } = unpackRig(bytes.subarray(1));
        const was = this.of(m.account);
        const lookWas = this.looks.get(m.account);
        this.looks.set(m.account, look);
        this.seen.set(m.account, { rig, look, at: Date.now(), bytes: bytes.length });
        // Avatars read the rig every frame: tell listeners only of a change
        // they'd draw differently (someone new, a new look).
        if (!was?.rig || lookWas?.character !== look.character || lookWas?.colour !== look.colour) this.emit();
        return;
      }
      const frame = await decodeFrame(bytes);
      this.seen.set(m.account, { frame, at: Date.now(), bytes: bytes.length });
      this.emit();
    } catch {
      // A frame this page can't read (a newer version): skipped.
    }
  }
}

function cameraRefusal(e: Error): string {
  if (e.name === "NotAllowedError") return "The camera wasn't allowed: allow it for this site in the browser, then try again.";
  if (e.name === "NotFoundError") return "No camera here.";
  if (!navigator.mediaDevices?.getUserMedia) return "This window can't use a camera (the app's webview may not offer one).";
  return `The camera didn't start: ${e.message}`;
}

export const outlines = new OutlineStore();
