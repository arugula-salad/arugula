// Panel (kit §4.1, 9): the agent panel on the right (Lift 4, #1480). A
// column: whatever a place puts over it (`above`: the Overview's Needs you),
// then the panel, which takes the rest. The panel is its tabs (the chief of
// staff first, a visitor in italics with its ×), a head (the agent's mark,
// name, state, rung and machine; Transcript or Terminal; ⤢ the floor), the
// body (a transcript or a well, with a thread drawn over it while one is
// open), and its foot (the composer, or why there is none).
//
// The frame owns its width (420 by default; `[` narrower to 320, `]` wider
// to 600, `\` a thin strip and back), kept per person in this browser (a
// place that starts it `folded`, a Doc's page, keeps its own fold); the
// floor (`f`: its body, the transcript or the terminal, over the whole page;
// ⤢ or Esc leaves it, and its tabs, head and composer stay where they were); and its keys,
// in the one key table: `l` focuses the panel, `h` the work beside it
// (`[data-work]`), `v` Transcript or Terminal. What a tab shows is the
// page's: `AgentPanel` (app/panel.tsx) draws agents, rooms and threads in
// it, and a place reaches that through `usePanel()`.

import { isValidElement, type ComponentChildren, type ComponentType, type VNode } from "preact";
import { useRef, useState } from "preact/hooks";
import { both, useBindings, type Binding } from "../keys";
import { usePlatformApi } from "../platform-api";
import { ChatMarkdown } from "../chat/chat-markdown";
import { surfacesFor } from "../surfaces";
import { claudeParts, partsOf } from "../transcript";
import { dialogUp } from "../ui/modal";
import { ChevronIcon, ExpandIcon, IconButton, MoreIcon } from "../ui/icons";
import { escapeTaken, menuBelow, openMenu, type MenuItem } from "../ui/menu";
import { Avatar } from "./avatar";
import { Kbd } from "./kbd";
import { StateMark, type State } from "./state-mark";
import { Surface } from "./surface";
import { Tabs, type TabItem } from "./tabs";
import type { PanelTarget } from "./panel-key";
import "./kit.css";

export { CHIEF_KEY, panelKey, type PanelTarget } from "./panel-key";

export interface AgentPanelProps {
  /** The project's chief of staff is always the first tab, added by the panel. */
  project: string;
  /** Rows over the panel (Needs you); the panel takes the rest of the column. */
  above?: ComponentChildren;
  /** The other tabs, in order. */
  targets: PanelTarget[];
  /** panelKey of the tab in front; undefined: the chief of staff. */
  current?: string;
  /** This place starts the panel as a strip (`\` opens it), whatever the
   * reader's fold elsewhere, which it leaves as it was: a Doc's author. */
  folded?: boolean;
  /** A tab picked: its panelKey, or CHIEF_KEY for the chief of staff. */
  onPick?: (key: string) => void;
  /** A tab's ×; without it, a target's tab has no ×. */
  onClose?: (key: string) => void;
  /** The chief of staff's tab as ✦ alone, to leave the width to the place's tab (a board, #1520). */
  chiefIcon?: boolean;
  /** A switch right after one tab (`key`, its panelKey): a board's ⇄ between its lead and its channel. */
  toggle?: { key: string; label: string; title: string; run: () => void };
}

/** The page's agent panel, where it has one (the app's, over its machines). */
export function usePanel(): ComponentType<AgentPanelProps> | null {
  return usePlatformApi().AgentPanel ?? null;
}

/** Who the open tab is, for the head. */
export interface PanelHead {
  name: string;
  /** Its state as a mark and a word (`word` says it differently: "asleep"). */
  state?: State;
  word?: string;
  /** What the state means for a message, as its tooltip. */
  hint?: string;
  /** "chief of staff", "lead", "worker", "room". */
  rung: string;
  machine?: string;
  /** The head's state for a spec (`data-panel-state`). */
  status?: string;
  /** A person or a room, not an agent: no ✻. */
  person?: boolean;
}

export type PanelView = "transcript" | "terminal";

const WIDTHS = [320, 420, 600] as const;
const STORE = "arugula.panel";

function readWidth(): { w: number; shut: boolean } {
  try {
    const v = JSON.parse(localStorage.getItem(STORE) ?? "null") as { w?: number; shut?: boolean } | null;
    if (v && WIDTHS.includes(v.w as (typeof WIDTHS)[number])) return { w: v.w!, shut: !!v.shut };
  } catch {
    // Unreadable: the default.
  }
  return { w: 420, shut: false };
}

/** Message tools and pickers that take Escape themselves. */
const OWNS_ESCAPE = "[data-react-menu], [data-msg-menu], [data-emoji-picker], .mention-pick, .menu, .prompt, [role=menu]";

export function Panel({
  label,
  tabs,
  current,
  onPick,
  onClose,
  head,
  view,
  onView,
  floors = true,
  back,
  above,
  foot,
  lede,
  over,
  flush,
  folded,
  children,
}: {
  label: string;
  /** None: no tab strip (a place that shows one conversation). */
  tabs?: TabItem[];
  current?: string;
  onPick?: (id: string) => void;
  onClose?: (id: string) => void;
  /** Who it is; or the place's own head, drawn as it is. */
  head: PanelHead | VNode;
  /** Transcript or Terminal, where the tab has both (`onView`). */
  view?: PanelView;
  onView?: (v: PanelView) => void;
  /** Whether it goes full screen (`f`). */
  floors?: boolean;
  /** Drilled into something (a thread): the way back, also Esc, and what
   * else it does in a ⋯ beside it (Follow). */
  back?: { label: string; go: () => void; more?: MenuItem[] };
  above?: ComponentChildren;
  foot?: ComponentChildren;
  /** Over the body, under the head: what the agent needs before you write to it (start it, restart it). */
  lede?: ComponentChildren;
  /** Drawn over the body, which stays as it was under it (a thread over its room). */
  over?: ComponentChildren;
  /** The body holds its own scroller (a room): no padding, no scroll of its own. */
  flush?: boolean;
  /** A strip at first, and its fold not kept: the place's own (AgentPanelProps'). */
  folded?: boolean;
  children: ComponentChildren;
}) {
  const [size, setSize] = useState(() => (folded === undefined ? readWidth() : { ...readWidth(), shut: folded }));
  const [floor, setFloor] = useState(false);
  const col = useRef<HTMLDivElement>(null);
  const put = (next: { w: number; shut: boolean }) => {
    setSize(next);
    try {
      localStorage.setItem(STORE, JSON.stringify(folded === undefined ? next : { ...next, shut: readWidth().shut }));
    } catch {
      // Not kept: it holds for this page.
    }
  };
  const step = (by: 1 | -1) => {
    const i = WIDTHS.indexOf(size.w as (typeof WIDTHS)[number]);
    put({ w: WIDTHS[Math.max(0, Math.min(WIDTHS.length - 1, i + by))]!, shut: false });
  };
  const focusPanel = () => requestAnimationFrame(() => col.current?.querySelector<HTMLElement>("[data-kit=panel]")?.focus());

  // The keys read the panel as it is now.
  const now = useRef({ size, floor, view, onView, back, floors });
  now.current = { size, floor, view, onView, back, floors };
  const keys: Binding[] = [
    {
      id: "panel-focus",
      label: "Focus the agent panel",
      chip: "panel",
      keys: both("key:l"),
      hint: { mac: "l h", other: "l h" },
      run: () => {
        if (now.current.size.shut) put({ ...now.current.size, shut: false });
        focusPanel();
      },
    },
    {
      id: "panel-work",
      label: "Focus the work beside the panel",
      keys: both("key:h"),
      run: () => (document.querySelector<HTMLElement>("[data-work]") ?? document.querySelector<HTMLElement>("main"))?.focus(),
    },
    {
      id: "panel-view",
      label: "Transcript or terminal, in the panel",
      keys: both("key:v"),
      when: () => !!now.current.onView && !now.current.size.shut,
      run: () => now.current.onView!(now.current.view === "terminal" ? "transcript" : "terminal"),
    },
    {
      id: "panel-floor",
      label: "The panel's transcript or terminal full screen, and back",
      keys: both("key:f"),
      when: () => now.current.floors && !now.current.size.shut,
      run: () => setFloor((f) => !f),
    },
    { id: "panel-narrow", label: "Narrower panel", keys: both("key:["), run: () => step(-1) },
    { id: "panel-wide", label: "Wider panel", keys: both("key:]"), run: () => step(1) },
    {
      id: "panel-collapse",
      label: "Collapse the panel to a strip, and back",
      keys: both("key:\\"),
      run: () => {
        setFloor(false);
        put({ ...now.current.size, shut: !now.current.size.shut });
      },
    },
    {
      id: "panel-back",
      label: "Leave the floor, or a thread in the panel",
      // From a field too (the floor's composer), but never a terminal's.
      keys: both("*+key:escape"),
      hint: { mac: "esc", other: "esc" },
      when: (e) =>
        (now.current.floor || !!now.current.back) &&
        !e.ctrlKey &&
        !e.metaKey &&
        !e.altKey &&
        !dialogUp() &&
        !escapeTaken(e) &&
        !document.querySelector(OWNS_ESCAPE) &&
        !document.activeElement?.closest(".xterm, .msg-edit"),
      run: () => (now.current.floor ? setFloor(false) : now.current.back?.go()),
    },
  ];
  useBindings(keys, []);

  if (size.shut)
    return (
      <div ref={col} class="k-panel-col is-shut" data-panel-col data-panel-shut="">
        <button type="button" class="k-panel-strip" aria-label="Open the agent panel" title="Open the agent panel" data-panel-open onClick={() => put({ ...size, shut: false })}>
          <ChevronIcon dir="left" size="sm" />
          <Kbd keys={"\\"} />
        </button>
      </div>
    );
  return (
    <div ref={col} class={floor ? "k-panel-col is-floor" : "k-panel-col"} style={{ "--panel-w": `${size.w}px` }} data-panel-col data-panel-width={size.w} data-panel-floor={floor ? "" : undefined}>
      {above}
      <Surface kind="section" as="aside" class="k-panel" aria-label={label} tabIndex={-1} data-kit="panel" data-panel={current}>
        <div class="k-panel-tabs">
          {tabs?.length ? <Tabs label={label} tabs={tabs} current={current ?? ""} onPick={onPick ?? (() => {})} onClose={onClose} /> : null}
          <IconButton class="k-panel-tool" label="Collapse the panel" title="Collapse the panel (\)" data-panel-collapse onClick={() => (setFloor(false), put({ ...size, shut: true }))}>
            <ChevronIcon dir="right" size="sm" />
          </IconButton>
        </div>
        {isValidElement(head) ? (
          <header class="k-panel-head">{head}</header>
        ) : (
        <header class="k-panel-head" data-panel-head={head.name} data-panel-state={head.status}>
          {!head.person && <Avatar name={head.name} agent />}
          <div class="k-panel-who">
            <b>{head.name}</b>
            <span class="k-panel-meta m-meta" title={head.hint}>
              {head.state && <StateMark state={head.state} word={head.word} />}
              {[head.rung, head.machine].filter(Boolean).map((x) => (
                <span key={x}>{x}</span>
              ))}
            </span>
          </div>
          {onView && (
            <div class="k-panel-views" role="group" aria-label="Show">
              {(["transcript", "terminal"] as const).map((v) => (
                <button key={v} type="button" aria-pressed={view === v} data-panel-view={v} onClick={() => onView(v)}>
                  {v === "transcript" ? "Transcript" : "Terminal"}
                </button>
              ))}
            </div>
          )}
          {floors && !floor && (
            <IconButton class="k-panel-tool" label="Full screen" title="Full screen (f)" aria-pressed={false} data-panel-floor-toggle onClick={() => setFloor(true)}>
              <ExpandIcon />
            </IconButton>
          )}
        </header>
        )}
        {back && (
          <div class="k-panel-back">
            <button type="button" data-panel-back onClick={back.go}>
              <ChevronIcon dir="left" size="xs" /> {back.label}
            </button>
            {back.more?.length ? (
              <IconButton
                class="k-panel-tool"
                label="More"
                title="More"
                data-panel-more
                onClick={(e) => {
                  const from = e.currentTarget;
                  openMenu(menuBelow(from, true), back.more!, { from, end: true, label: back.label });
                }}
              >
                <MoreIcon />
              </IconButton>
            ) : null}
          </div>
        )}
        {lede && <div class="k-panel-lede">{lede}</div>}
        <div class={flush ? "k-panel-body is-flush" : "k-panel-body"} data-panel-body>
          {floor && (
            <IconButton class="k-panel-tool k-panel-unfloor" label="Leave full screen" title="Leave full screen (f, esc)" aria-pressed data-panel-floor-toggle onClick={() => setFloor(false)}>
              <ExpandIcon />
            </IconButton>
          )}
          {children}
          {over && <div class="k-panel-over">{over}</div>}
        </div>
        {foot && <div class="k-panel-foot">{foot}</div>}
      </Surface>
    </div>
  );
}

/** A line where the composer would be, or at a transcript's end: why there's nothing to write in. */
export function PanelNote({ children, end, ...data }: { children: ComponentChildren; end?: boolean } & { [d: `data-${string}`]: unknown }) {
  return (
    <p class={end ? "k-panel-note is-end m-meta" : "k-panel-note"} {...data}>
      {children}
    </p>
  );
}

/** An agent's transcript (its Markdown; `claude`: Claude Code's screen in a
 * terminal): prose as chat draws a message, sans with ids and code in mono,
 * and each tool call folded to its one line, which opens; with
 * surfacesFor's `said`, no tool calls. */
export function Transcript({ text, claude, ...data }: { text: string; claude?: boolean } & { [d: `data-${string}`]: unknown }) {
  const parts = (claude ? claudeParts : partsOf)(text, surfacesFor().transcript);
  return (
    <div class="k-panel-transcript" {...data}>
      {parts.map((p, i) =>
        p.kind === "prose" ? (
          <ChatMarkdown key={i} text={p.text} />
        ) : (
          <details key={i} class="k-panel-tool-call" data-panel-tool>
            <summary>{p.line.replace(/\*\*/g, "").replace(/`/g, "")}</summary>
            {p.body && <pre>{p.body}</pre>}
          </details>
        ),
      )}
    </div>
  );
}
