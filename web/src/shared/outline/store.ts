// Who's around as outlines (S43), for every People block on the page: one
// stream from control of the frames of those you share a team with, and
// your own camera, if you turned it on. Control relays frames and keeps
// only each person's latest, in memory (crates/platform/src/chat/outlines.rs).
//
// The stream runs while a block that shows it is open (`watch`), and the
// camera runs until you turn it off or close the page; nothing turns it
// on but you.

import { platformFetch } from "../maintenance";
import { startCamera, type Camera, type FrameStats } from "./camera";
import { decodeFrame, type Frame } from "./frame";

/** A frame older than this is someone whose camera went away. */
const STALE = 12_000;

export interface Seen {
  frame: Frame;
  at: number;
  bytes: number;
}

export interface Mine {
  frame: Frame | null;
  stats: FrameStats | null;
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
  private starting = false;
  fps = 2;
  detail = 0.5;
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

  /** A block shows outlines: the stream runs while one does. */
  watch(): () => void {
    if (this.watchers++ === 0) this.open();
    return () => {
      if (--this.watchers === 0) this.close();
    };
  }

  get cameraOn(): boolean {
    return !!this.camera || this.starting;
  }

  async startCamera() {
    if (this.camera || this.starting) return;
    this.starting = true;
    this.mine = { frame: null, stats: null, sent: [], error: "" };
    this.emit();
    try {
      this.camera = await startCamera({
        fps: this.fps,
        detail: this.detail,
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
      this.mine = { frame: null, stats: null, sent: [], error: cameraRefusal(e as Error) };
    } finally {
      this.starting = false;
      this.emit();
    }
  }

  stopCamera() {
    if (!this.camera) return;
    this.camera.stop();
    this.camera = null;
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
