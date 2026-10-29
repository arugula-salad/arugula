// Who's around, for every view of them on the page (Arugula S43, S44): the
// frames of the people in the room, heard through a transport (./transport:
// arugula.video's relay, or Arugula control's), and your own camera, if
// you turned it on. Framework-free: a view subscribes and reads.
//
// Hearing runs while a view that shows people is open (`watch`), and the
// camera runs until you turn it off or close the page; nothing turns it
// on but you.
//
// A camera sends outlines or, in avatar mode, rigs (presence-core's
// avatar): your pose as numbers, which every page draws as the character
// you picked. A rig comes a dozen times a second and avatars read it every
// animation frame, so a rig only tells listeners when someone starts or
// stops.

import { startCamera, type Camera, type FrameStats } from "@arugula/presence-core/outline/camera";
import { decodeFrame, type Frame } from "@arugula/presence-core/outline/frame";
import { RIG, WAVE_MS, unpackRig, type Look, type Rig } from "@arugula/presence-core/avatar/rig";
import type { AvatarCamera } from "@arugula/presence-core/avatar/cam";
import type { Member, Mode, Transport, TransportEvents } from "./transport.ts";

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

export interface PresenceClientOptions {
  transport: Transport;
  /** Where this browser keeps your mode and look (localStorage keys
   * `<storage>mode`, `<storage>look`). */
  storage?: string;
}

export class PresenceClient {
  private seen = new Map<string, Seen>();
  private listeners = new Set<() => void>();
  private transport: Transport;
  private storage: string;
  private watchers = 0;
  private sweep: number | undefined;
  private camera: Camera | null = null;
  private avatarCam: AvatarCamera | null = null;
  private starting = false;
  /** The look each person last sent, so their avatar idles in it after
   * their camera goes off. */
  private looks = new Map<string, Look>();
  /** Outline lines or an avatar (S44); yours, kept in this browser. */
  mode: "outline" | "avatar";
  /** Your character and colour (S44). */
  look: Look;
  /** Rig frames a second. */
  rigFps = 12;
  fps = 4;
  detail = 0.7;
  /** Colour lines by what they trace (presence-core's outline/labels). */
  label = true;
  /** Labelling's state, and why it stopped if it did. */
  labelState: "loading" | "on" | "off" = "off";
  labelError = "";
  /** Estimate depth (presence-core's outline/depth): far lines fainter, labels checked. */
  depth = true;
  depthState: "loading" | "on" | "off" = "off";
  depthError = "";
  mine: Mine | null = null;
  /** Hearing others, for a view to say. */
  live: "off" | "connecting" | "on" | "refused" = "off";
  /** Who's in the room, where the transport knows (the relay's presence). */
  members: Member[] = [];
  /** Who waved, newest last, until answered. */
  waves: { from: string; at: number }[] = [];
  private wavedAt = 0;
  /** Answers to your waves, newest last, until dismissed. */
  answers: { from: string; yes: boolean; at: number }[] = [];

  constructor(o: PresenceClientOptions) {
    this.transport = o.transport;
    this.storage = o.storage ?? "arugula.presence.";
    this.mode = kept(`${this.storage}mode`, "outline");
    this.look = kept(`${this.storage}look`, { character: 2, colour: 0 });
  }

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
    keep(`${this.storage}mode`, mode);
    this.emit();
    if (was) void this.startCamera();
  }

  setLook(look: Look) {
    this.look = look;
    keep(`${this.storage}look`, look);
    this.avatarCam?.setLook(look);
    this.transport.state?.({ look });
    this.emit();
  }

  /** Say whether you're away (the relay's presence carries it). */
  setAway(away: boolean) {
    this.transport.state?.({ away });
  }

  /** Ask `to` to talk, or answer someone who asked. */
  wave(to: string) {
    this.wavedAt = Date.now();
    this.transport.wave?.(to);
  }

  /** Whether `sub`'s avatar is waving now: their wave (or wave back) came
   * in the last WAVE_MS, or it's you and you just waved. */
  waving(sub: string, me = false): boolean {
    const fresh = (at: number) => Date.now() - at < WAVE_MS;
    if (me) return fresh(this.wavedAt);
    return this.waves.some((w) => w.from === sub && fresh(w.at)) || this.answers.some((a) => a.from === sub && a.yes && fresh(a.at));
  }

  answer(from: string, yes: boolean) {
    if (yes) this.wavedAt = Date.now();
    this.waves = this.waves.filter((k) => k.from !== from);
    this.transport.answer?.(from, yes);
    this.emit();
  }

  /** Forget an answer to your wave (it's been seen). */
  dismiss(from: string) {
    this.answers = this.answers.filter((a) => a.from !== from);
    this.emit();
  }

  /** A view shows people: hearing runs while one does. */
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
    this.transport.state?.({ mode: this.mode as Mode, look: this.look });
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
      this.transport.state?.({ mode: "none" });
    } finally {
      this.starting = false;
      this.emit();
    }
  }

  private async startAvatar() {
    let told = 0;
    try {
      const { startAvatarCamera } = await import("@arugula/presence-core/avatar/cam");
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
      this.transport.state?.({ mode: "none" });
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
    this.transport.off();
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
    this.transport.leaving?.();
  };

  private post(bytes: Uint8Array) {
    this.transport.publish(bytes);
  }

  private events: TransportEvents = {
    frame: (sub, bytes) => void this.frame(sub, bytes),
    off: (sub) => {
      this.seen.delete(sub);
      this.emit();
    },
    live: (state) => {
      this.live = state;
      this.emit();
    },
    members: (members) => {
      this.members = members;
      this.emit();
    },
    wave: (from) => {
      this.waves = [...this.waves.filter((k) => k.from !== from), { from, at: Date.now() }];
      this.emit();
    },
    answer: (from, yes) => {
      this.answers = [...this.answers.filter((a) => a.from !== from), { from, yes, at: Date.now() }];
      this.emit();
    },
  };

  private open() {
    this.transport.listen(this.events);
    // Someone whose frames stopped without an `off` (a closed laptop).
    this.sweep = setInterval(() => this.emit(), 3_000) as unknown as number;
    this.emit();
  }

  private close() {
    this.transport.unlisten();
    clearInterval(this.sweep);
    this.seen.clear();
    this.live = "off";
    this.emit();
  }

  /** `sub`'s frame off the transport: a rig (an avatar) or an outline. */
  private async frame(sub: string, bytes: Uint8Array) {
    try {
      if (bytes[0] === 0 && bytes[1] === RIG) {
        const { rig, look } = unpackRig(bytes.subarray(1));
        const was = this.of(sub);
        const lookWas = this.looks.get(sub);
        this.looks.set(sub, look);
        this.seen.set(sub, { rig, look, at: Date.now(), bytes: bytes.length });
        // Avatars read the rig every frame: tell listeners only of a change
        // they'd draw differently (someone new, a new look).
        if (!was?.rig || lookWas?.character !== look.character || lookWas?.colour !== look.colour) this.emit();
        return;
      }
      const frame = await decodeFrame(bytes);
      this.seen.set(sub, { frame, at: Date.now(), bytes: bytes.length });
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

