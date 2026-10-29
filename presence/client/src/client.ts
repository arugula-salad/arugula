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
import type { CallJoin, Member, Message, Mode, Transport, TransportEvents } from "./transport.ts";
import { joinLiveKit, type CallPerson, type JoinCall, type LiveCall } from "./call.ts";

/** The most messages a page keeps (older ones are a loadEarlier() away). */
export const MESSAGES_KEPT = 500;
/** A history page from the relay (PROTOCOL.md). */
const PAGE = 50;

/** `into` with `more` merged in: by id, oldest first, the newest
 * MESSAGES_KEPT. */
export function mergeMessages(into: Message[], more: Message[]): Message[] {
  const byId = new Map(into.map((m) => [m.id, m]));
  for (const m of more) byId.set(m.id, m);
  return [...byId.values()].sort((a, b) => a.id - b.id).slice(-MESSAGES_KEPT);
}

/** How long a reaction floats over its sender's tile. */
export const REACT_MS = 2400;

/** The reactions a page offers (any single emoji goes on the wire). */
export const REACTIONS = ["👍", "👏", "😂", "🎉", "❤️", "👋", "☕"] as const;

/** A call you're in, or ringing someone into (PROTOCOL.md "Calls"). */
export interface Call {
  id: string;
  /** Ringing someone (nobody's said yes yet), joining LiveKit, in it, or
   * LiveKit reconnecting by itself. */
  state: "ringing" | "joining" | "on" | "reconnecting";
  /** Who you're ringing now (into this call). */
  ringing: string[];
  /** Who's in it, you included, as LiveKit says (once joined). */
  people: CallPerson[];
  muted: boolean;
  /** The browser blocked the call's audio until a click (`startAudio`). */
  blocked: boolean;
  error: string;
}

/** In a call that's on, alone this long (everyone else left, or nobody
 * came), and it ends. */
const ALONE_MS = 30_000;

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
  /** Tests: how a call's LiveKit room is joined. */
  joinCall?: JoinCall;
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
  /** The room's text messages, oldest first (PROTOCOL.md `say`). */
  messages: Message[] = [];
  /** Whether there may be messages before the first one here. */
  hasEarlier = true;
  /** Reactions in the last REACT_MS, yours included, oldest first: an
   * emoji floating over someone's tile. */
  reactions: { from: string; emoji: string; at: number }[] = [];
  /** Answers to your waves and calls, newest last, until dismissed. */
  answers: { from: string; yes: boolean; at: number }[] = [];
  /** Whether the relay has calls (PROTOCOL.md "Calls"): hide call
   * buttons where it hasn't. */
  callsOn = false;
  /** Who's ringing you, newest last, until answered or over. */
  rings: { from: string; id: string; at: number }[] = [];
  /** The call you're in or ringing someone into. */
  inCall: Call | null = null;
  private live_: LiveCall | null = null;
  private joinCall: JoinCall;
  private alone: number | undefined;
  private rejoined = false;

  constructor(o: PresenceClientOptions) {
    this.transport = o.transport;
    this.storage = o.storage ?? "arugula.presence.";
    this.mode = kept(`${this.storage}mode`, "outline");
    this.look = kept(`${this.storage}look`, { character: 2, colour: 0 });
    this.joinCall = o.joinCall ?? joinLiveKit;
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

  /** Send a text message to the room; resolves with it as kept, rejects
   * with the relay's reason (too long, too fast…). */
  async say(text: string): Promise<Message> {
    if (!this.transport.say) throw new Error("this room has no text");
    const m = await this.transport.say(text);
    this.messages = mergeMessages(this.messages, [m]);
    this.emit();
    return m;
  }

  /** The page of messages before the first one here. */
  async loadEarlier(): Promise<void> {
    const first = this.messages[0];
    if (!first || !this.transport.history || !this.hasEarlier) return;
    const earlier = await this.transport.history(first.id);
    this.hasEarlier = earlier.length >= PAGE;
    this.messages = mergeMessages(earlier, this.messages);
    this.emit();
  }

  /** Send an emoji to the room (`me` is your sub, for your own tile). A
   * 👋 waves on your avatar too. */
  react(emoji: string, me = "") {
    this.transport.react?.(emoji);
    this.reacted(me, emoji);
    if (emoji === "👋") this.wavedAt = Date.now();
  }

  /** The reactions over `sub`'s tile now. */
  reactionsOf(sub: string): { emoji: string; at: number }[] {
    return this.reactions.filter((r) => r.from === sub && Date.now() - r.at < REACT_MS);
  }

  private reacted(from: string, emoji: string) {
    const now = Date.now();
    this.reactions = [...this.reactions.filter((r) => now - r.at < REACT_MS), { from, emoji, at: now }];
    this.emit();
    // Gone from view once it's floated away.
    setTimeout(() => this.emit(), REACT_MS + 50);
  }

  /** Whether `sub`'s avatar is waving now: their wave (or wave back) came
   * in the last WAVE_MS, or it's you and you just waved. */
  waving(sub: string, me = false): boolean {
    const fresh = (at: number) => Date.now() - at < WAVE_MS;
    if (me) return fresh(this.wavedAt);
    return (
      this.waves.some((w) => w.from === sub && fresh(w.at)) ||
      this.answers.some((a) => a.from === sub && a.yes && fresh(a.at)) ||
      this.rings.some((r) => r.from === sub && fresh(r.at)) ||
      this.reactions.some((r) => r.from === sub && r.emoji === "👋" && fresh(r.at))
    );
  }

  answer(from: string, yes: boolean) {
    if (yes) this.wavedAt = Date.now();
    this.waves = this.waves.filter((k) => k.from !== from);
    this.transport.answer?.(from, yes);
    this.emit();
  }

  /** Ring `to` to talk (into your call, if you're in one): your avatar
   * waves, theirs rings. Resolves with the call's id; rejects with the
   * relay's reason ("calls aren't set up here", "not in the room"…). */
  async call(to: string): Promise<string> {
    if (!this.transport.call) throw new Error("calls aren't set up here");
    this.wavedAt = Date.now();
    this.emit();
    const { id } = await this.transport.call(to);
    const c = this.inCall;
    this.inCall = c && c.id === id ? { ...c, ringing: [...c.ringing.filter((s) => s !== to), to] } : { id, state: "ringing", ringing: [to], people: [], muted: false, blocked: false, error: "" };
    this.emit();
    return id;
  }

  /** Answer a ring: yes joins the call (leaving any other). */
  async answerCall(id: string, yes: boolean): Promise<void> {
    this.rings = this.rings.filter((r) => r.id !== id);
    if (yes) this.wavedAt = Date.now();
    this.emit();
    await this.transport.callAnswer?.(id, yes);
  }

  /** Leave the call (or stop ringing). */
  leaveCall() {
    const c = this.inCall;
    if (!c) return;
    this.transport.callLeave?.(c.id);
    this.endCall();
  }

  async setMuted(muted: boolean) {
    if (!this.inCall) return;
    this.inCall = { ...this.inCall, muted };
    this.emit();
    await this.live_?.setMuted(muted);
  }

  /** From a click: play a call the browser wouldn't play by itself. */
  async startAudio() {
    await this.live_?.startAudio();
    if (this.inCall) this.inCall = { ...this.inCall, blocked: false };
    this.emit();
  }

  /** Whether `sub` is in a call (presence says), and in yours. */
  inACall(sub: string): boolean {
    return !!this.members.find((m) => m.sub === sub)?.call;
  }

  inMyCall(sub: string): boolean {
    const c = this.inCall;
    return !!c && c.state !== "ringing" && this.members.find((m) => m.sub === sub)?.call === c.id;
  }

  private endCall() {
    const live = this.live_;
    this.live_ = null;
    this.inCall = null;
    clearTimeout(this.alone);
    void live?.leave();
    this.emit();
  }

  private patchCall(id: string, change: Partial<Call>) {
    if (this.inCall?.id !== id) return;
    this.inCall = { ...this.inCall, ...change };
    this.emit();
  }

  /** Into call `j.id`'s LiveKit room (out of any other first). */
  private async joined(j: CallJoin) {
    if (this.inCall?.id !== j.id) {
      void this.live_?.leave();
      this.live_ = null;
    }
    const was = this.inCall?.id === j.id ? this.inCall : null;
    this.inCall = { id: j.id, state: "joining", ringing: was?.ringing ?? [], people: [], muted: was?.muted ?? false, blocked: false, error: "" };
    this.emit();
    // Nobody else turns up: it ends.
    const lonely = (people: CallPerson[]) => {
      clearTimeout(this.alone);
      if (people.length <= 1) this.alone = setTimeout(() => this.inCall?.id === j.id && this.leaveCall(), ALONE_MS) as unknown as number;
    };
    let had = 0;
    try {
      const live = await this.joinCall(j, {
        people: (people) => {
          // Everyone else left: so does this page.
          if (had > 1 && people.length <= 1) return this.inCall?.id === j.id && this.leaveCall();
          had = Math.max(had, people.length);
          lonely(people);
          this.patchCall(j.id, { people });
        },
        state: (state, byUs) => {
          if (state !== "ended") return this.patchCall(j.id, { state });
          if (byUs || this.inCall?.id !== j.id) return;
          void this.rejoin(j.id);
        },
        blocked: (blocked) => this.patchCall(j.id, { blocked }),
      });
      if (this.inCall?.id !== j.id) return void live.leave();
      this.live_ = live;
      this.rejoined = false;
      if (this.inCall.muted) await live.setMuted(true);
      lonely(this.inCall.people);
    } catch (e) {
      this.patchCall(j.id, { error: `The call didn't connect: ${(e as Error).message}` });
    }
  }

  /** LiveKit dropped this page: once, back in with a fresh token. */
  private async rejoin(id: string) {
    this.live_ = null;
    if (this.rejoined || !this.transport.callToken) return this.leaveCall();
    this.rejoined = true;
    try {
      await this.joined(await this.transport.callToken(id));
    } catch {
      this.leaveCall();
    }
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
    react: (from, emoji) => this.reacted(from, emoji),
    said: (m) => {
      this.messages = mergeMessages(this.messages, [m]);
      this.emit();
    },
    history: (messages) => {
      if (!this.messages.length) this.hasEarlier = messages.length >= PAGE;
      this.messages = mergeMessages(this.messages, messages);
      this.emit();
    },
    answer: (from, yes) => {
      this.answers = [...this.answers.filter((a) => a.from !== from), { from, yes, at: Date.now() }];
      this.emit();
    },
    calls: (on) => {
      this.callsOn = on;
      this.emit();
    },
    call: (from, id) => {
      this.rings = [...this.rings.filter((r) => r.id !== id && r.from !== from), { from, id, at: Date.now() }];
      this.emit();
    },
    callCancel: (id) => {
      this.rings = this.rings.filter((r) => r.id !== id);
      this.emit();
    },
    callAnswer: (id, from, yes) => {
      // A call's answer reads like a wave's (waved back, not now).
      this.answers = [...this.answers.filter((a) => a.from !== from), { from, yes, at: Date.now() }];
      const c = this.inCall;
      if (c?.id === id) {
        const ringing = c.ringing.filter((s) => s !== from);
        // Nobody said yes to a call that hadn't started: it's over.
        if (!yes && c.state === "ringing" && !ringing.length) this.inCall = null;
        else this.inCall = { ...c, ringing };
      }
      this.emit();
    },
    callJoin: (j) => void this.joined(j),
  };

  private open() {
    this.transport.listen(this.events);
    // Someone whose frames stopped without an `off` (a closed laptop).
    this.sweep = setInterval(() => this.emit(), 3_000) as unknown as number;
    this.emit();
  }

  private close() {
    this.leaveCall();
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

