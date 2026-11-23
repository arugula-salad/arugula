// The Overview's rows (Arugula #1086, #1087): one line per card in flight,
// its number, its title cut to one line, the one word of how it stands
// (../shell/swarm/overview.ts) and its assignees' faces; and a session on no
// card, one line too. Hovering a row, or focusing it from the keyboard,
// opens its preview under it: each session on it, where it runs and its
// pane's last line, read from its machine only while the preview is open.
// A click on a row, or a session in a preview, peeks at it in the talk
// panel (./overview-peek.tsx). An open board's row (#1085) holds them.

import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { MsgAvatar } from "../shared/chat/avatar";
import { isCard, pointerLabel } from "../shared/boards/text";
import type { Tone } from "../shared/ui/state-chip";
import type { Fleet } from "../shell/fleet";
import { shortHost } from "../shell/swarm/model";
import { wordOf, type OvCard, type OvPane, type OvSession, type Standing } from "../shell/swarm/overview";

/** How often an open preview reads its last lines again. */
const LINE_EVERY_MS = 6000;
/** How long the pointer rests on a row before its preview opens, so passing over rows opens none. */
const HOVER_MS = 250;

/** The colour of a row's word: what waits on you, what failed, what waits on someone else, what's quiet; the rest plain. */
const TONE: Partial<Record<Standing, Tone>> = { needs: "accent", asks: "accent", failed: "danger", checks_failing: "danger", waits: "info", quiet: "warn" };

type Data = { [data: `data-${string}`]: unknown };

/** A row: its line (a button that opens it) and, while it's hovered or
 * focused, `preview` under it. The preview is drawn only while open, so
 * what it reads is read only then. */
export function OvRow({ line, open, disabled, title, current, preview, class: cls, ...attrs }: { line: ComponentChildren; open: () => void; disabled?: boolean; title?: string; current?: boolean; preview?: () => ComponentChildren; class?: string } & Data) {
  const [focused, setFocused] = useState(false);
  const [hovered, setHovered] = useState(false);
  const me = useRef<HTMLLIElement>(null);
  const rest = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // A click focuses the row too: only focus that isn't a press (the keyboard's) opens it.
  const pressed = useRef(false);
  useEffect(() => () => clearTimeout(rest.current), []);
  const on = (focused || hovered) && !!preview;
  return (
    <li
      ref={me}
      class={`ov-row${cls ? ` ${cls}` : ""}${current ? " ov-row-current" : ""}${on ? " ov-row-open" : ""}`}
      onPointerEnter={(e) => {
        if (e.pointerType !== "touch") rest.current = setTimeout(() => setHovered(true), HOVER_MS);
      }}
      onPointerLeave={() => {
        clearTimeout(rest.current);
        setHovered(false);
      }}
      onPointerDown={() => (pressed.current = true)}
      onFocusIn={() => {
        if (!pressed.current) setFocused(true);
        pressed.current = false;
      }}
      onFocusOut={(e) => !me.current?.contains(e.relatedTarget as Node | null) && setFocused(false)}
      onKeyDown={(e) => {
        if (e.key === "Escape" && on) {
          e.stopPropagation();
          setFocused(false);
          setHovered(false);
        }
      }}
      {...attrs}
    >
      <button type="button" class="ov-row-line" disabled={disabled} title={title} aria-current={current ? "true" : undefined} aria-expanded={preview ? on : undefined} onClick={open} data-ov-row-open>
        {line}
      </button>
      {on && (
        <div class="ov-preview" data-ov-preview>
          {preview!()}
        </div>
      )}
    </li>
  );
}

/** A row's one word, in its colour; nothing when nothing says. `asks`
 * names whom it waits on (`asks Aaron`, #1392); `words` says it otherwise,
 * in the same colour (a board's row: `2 working`). */
export function Word({ standing, asks, words, ...attrs }: { standing: Standing; asks?: string; words?: string } & Data) {
  const word = words ?? wordOf({ standing, asks });
  if (!word) return null;
  return (
    <span class={`ov-word tone-${TONE[standing] ?? "plain"}`} {...attrs}>
      {word}
    </span>
  );
}

/** A card's line, and its sessions in its preview. `faceOf` finds an assignee's account, for their picture. */
export function CardRow({ c, fleet, open, openSession, current, faceOf }: { c: OvCard; fleet: Fleet; open: () => void; openSession: (s: OvSession) => void; current?: boolean; faceOf: (name: string) => string | undefined }) {
  const r = c.row;
  const label = pointerLabel(r.pointer, r.live);
  const title = isCard(r.live) ? r.live.title : r.note.split("\n").find((l) => l.trim()) ?? label;
  const who = isCard(r.live) ? r.live.assignees ?? [] : [];
  return (
    <OvRow
      class={c.standing === "needs" ? "ov-pulse" : undefined}
      current={current}
      open={open}
      title={title}
      data-ov-card={r.id}
      data-ov-standing={c.standing}
      data-ov-panes={c.sessions.flatMap((s) => s.panes.map((p) => p.key)).join(" ")}
      line={
        <>
          {label && <span class="ov-row-num">{label}</span>}
          <span class="ov-row-title" data-ov-card-title>
            {title}
          </span>
          <Word standing={c.standing} asks={c.asks} data-ov-card-state />
          {who.length > 0 && (
            <span class="ov-row-who" title={`Assigned to ${who.join(", ")}`}>
              {who.slice(0, 2).map((n) => (
                <MsgAvatar key={n} who={faceOf(n) ?? n} name={n} account={faceOf(n)} />
              ))}
            </span>
          )}
        </>
      }
      preview={
        c.sessions.length
          ? () =>
              c.sessions.map((s) => (
                <button key={s.key} type="button" class="ov-peek" title={`${s.name}: peek at it`} data-ov-session={s.key} data-ov-session-standing={s.standing} data-ov-panes={s.panes.map((p) => p.key).join(" ")} onClick={() => openSession(s)}>
                  <Peek s={s} fleet={fleet} />
                </button>
              ))
          : undefined
      }
    />
  );
}

/** A session on no card: its name, its word and where it runs; its last line in its preview. */
export function SessionRow({ s, fleet, open }: { s: OvSession; fleet: Fleet; open: () => void }) {
  return (
    <OvRow
      open={open}
      title={s.lead || s.hosted ? `${s.name}: peek at it` : `${s.name}, on ${s.where}: not reachable from here`}
      data-ov-session={s.key}
      data-ov-session-standing={s.standing}
      data-ov-panes={s.panes.map((p) => p.key).join(" ")}
      line={
        <>
          <span class="ov-row-title">{s.name}</span>
          <Word standing={s.standing} asks={s.asks} />
          <span class="ov-row-where" data-ov-where>
            {shortHost(s.where)}
          </span>
        </>
      }
      preview={
        s.lead
          ? () => (
              <div class="ov-peek">
                <Peek s={s} fleet={fleet} />
              </div>
            )
          : undefined
      }
    />
  );
}

/** A session in a preview: its agent and title, where it runs, and its last line. */
function Peek({ s, fleet }: { s: OvSession; fleet: Fleet }) {
  return (
    <>
      <span class="ov-peek-head">
        <b>{s.name}</b>
        <span class="dim" data-ov-where>
          {shortHost(s.where)}
        </span>
      </span>
      {s.lead ? <LastLine fleet={fleet} pane={s.lead} /> : <span class="ov-line dim">{wordOf(s)}</span>}
    </>
  );
}

/** A pane's last line, read from its machine while it's shown (a preview is open) and the page is. */
function LastLine({ fleet, pane }: { fleet: Fleet; pane: OvPane }) {
  const p = fleet.panes.find((x) => x.key === pane.key);
  const fallback = p?.info.reason?.headline ?? p?.info.current?.text ?? "";
  const [line, setLine] = useState<string | null>(null);
  useLayoutEffect(() => setLine(null), [pane.key]);
  useEffect(() => {
    let live = true;
    const read = async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const res = await fleet.request(pane.host, "GET", `/api/panes/${pane.id}/capture?format=text`);
        const text = res.ok && res.text ? await res.text() : "";
        const last = text
          .split("\n")
          .map((l) => l.trimEnd())
          .filter(Boolean)
          .pop();
        if (live && last) setLine(last);
      } catch {
        // Its machine isn't reachable now: the line it had stays.
      }
    };
    void read();
    const t = setInterval(() => void read(), LINE_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [pane.key]);
  return (
    <span class="ov-line" data-ov-line>
      {line ?? fallback}
    </span>
  );
}
