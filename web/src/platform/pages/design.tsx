// The design reference (#693, rebuilt as the kit's in Lift 2, #1478): every
// part of shared/kit/ drawn by the real component, in the order of
// docs/design/language.md, one section each with an anchor (`/design#row`).
// Look (the theme and the three sliders) repaints this page only; leaving
// puts the person's own back.
//
// It needs no one signed in and no project, and imports only shared/ and
// platform/. The tokens are read from tokens.css as the page draws them, so
// a new role shows up without editing this page.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { apply as applyAppearance, palette } from "../../shared/appearance";
import { Avatar } from "../../shared/kit/avatar";
import { Button } from "../../shared/kit/button";
import { Composer } from "../../shared/kit/composer";
import { Crumb } from "../../shared/kit/crumb";
import { Choice, TextField, Toggle } from "../../shared/kit/field";
import { Kbd } from "../../shared/kit/kbd";
import { paint as paintLook, sliders as startAt, type Sliders } from "../../shared/kit/look";
import { NeedList, NeedRow, type Need, type Receipt } from "../../shared/kit/need-row";
import { Panel, PanelNote } from "../../shared/kit/panel";
import { Row } from "../../shared/kit/row";
import { Sheet } from "../../shared/kit/sheet";
import { MARKS, StateMark, type State } from "../../shared/kit/state-mark";
import { Surface } from "../../shared/kit/surface";
import { Tabs, type TabItem } from "../../shared/kit/tabs";
import tokenSource from "../../shared/kit/tokens.css?raw";
import { Well } from "../../shared/kit/well";

// ---- read from the source

/** The role tokens: every custom property tokens.css's first `:root` sets,
 * by group (its comments' first words). */
const ROLES = [...(tokenSource.split("/* Today's names")[0] ?? "").matchAll(/^\s+(--[\w-]+):/gm)].map((m) => m[1]!);
const COLOURS = ROLES.filter((r) => !/^--(font|text|radius|gap|row|rail-w|panel-w|shadow|highlight|sheen|glow)/.test(r));
const SIZES = ROLES.filter((r) => r.startsWith("--text-"));

// ---- the page's pieces

function Section({ id, title, use, children }: { id: string; title: string; use: string; children: ComponentChildren }) {
  return (
    <section class="design-section" id={id} data-design-section={id} aria-labelledby={`${id}-title`}>
      <h2 id={`${id}-title`}>{title}</h2>
      <p class="design-use" data-design-use>
        {use} <code data-design-import>shared/kit/{id === "tokens" ? "tokens.css" : `${id}.tsx`}</code>
      </p>
      <div class="design-show">{children}</div>
    </section>
  );
}

const noop = () => {};

const NEEDS: Need[] = [
  {
    kind: "ask",
    title: "Crumb at 900px: truncate the middle, or hide the project?",
    from: { agent: "claude", machine: "geek" },
    for: "you",
    subject: { kind: "issue", id: "#1443" },
    lines: ["At 900px the crumb's last step is cut. Which goes first?"],
    answer: { via: "card", options: [
      { id: "middle", label: "Truncate middle" },
      { id: "project", label: "Hide the project" },
    ] },
    age: "7m",
  },
  {
    kind: "permission",
    title: "Allow Bash: rm -rf web/node_modules/.vite",
    from: { agent: "claude", machine: "dust" },
    for: "you",
    subject: { kind: "issue", id: "#1448" },
    lines: ["rm -rf web/node_modules/.vite", "Clears Vite's cache after the lockfile moved."],
    age: "20m",
  },
  { kind: "approval", title: "Start a worker on #1412 on geek?", from: { name: "chief of staff" }, for: "you", lines: ["It would branch from main."], age: "2m" },
  {
    kind: "todo",
    title: "Run the migration with prod creds",
    from: { agent: "claude", machine: "dust" },
    for: "you",
    subject: { kind: "issue", id: "#1430" },
    lines: ["The migration needs the prod role, which agents don't hold."],
    answer: { via: "todo" },
    how: "cd server\nARUGULA_ENV=prod just migrate",
    age: "1h",
  },
  { kind: "ask", title: "When do you and Jake run the rehearsal?", from: { agent: "claude", machine: "dust" }, for: "Jake", subject: { kind: "issue", id: "#1408" }, age: "9h" },
];

function NeedRows() {
  const [open, setOpen] = useState<number | null>(0);
  const [done, setDone] = useState<Record<number, Receipt>>({});
  return (
    <div class="design-third" data-design-need-width>
      <NeedList label="Needs you">
        {NEEDS.map((n, i) => (
          <NeedRow
            key={i}
            need={n}
            open={open === i}
            receipt={done[i]}
            onToggle={() => setOpen(open === i ? null : i)}
            onAnswer={(o, note) => setDone({ ...done, [i]: { what: o.label, by: "you", at: "now", note } })}
          />
        ))}
      </NeedList>
    </div>
  );
}

const TABS: TabItem[] = [
  { id: "chief", title: "Chief of staff" },
  { id: "1443", title: "#1443 worker", count: 1, closes: true },
  { id: "lead", title: "Launch: engineering lead", closes: true },
];

function TabsDemo() {
  const [tabs, setTabs] = useState(TABS);
  const [at, setAt] = useState("chief");
  return <Tabs label="Agents" tabs={tabs} current={at} onPick={setAt} onClose={(id) => (setTabs(tabs.filter((t) => t.id !== id)), at === id && setAt("chief"))} />;
}

function PanelDemo() {
  const [at, setAt] = useState("chief");
  const [view, setView] = useState<"transcript" | "terminal">("transcript");
  const tabs: TabItem[] = [
    { id: "chief", title: "Chief of staff" },
    { id: "1431", title: "#1431 staging checkout", closes: true, peek: true },
  ];
  return (
    <Panel
      label="Agent panel"
      tabs={tabs}
      current={at}
      onPick={setAt}
      onClose={() => setAt("chief")}
      head={at === "chief" ? { name: "Chief of staff", state: "working", rung: "chief of staff", machine: "dust" } : { name: "#1431 staging checkout", state: "idle", word: "ended 3h ago", rung: "worker", machine: "ws-2" }}
      view={view}
      onView={setView}
      floors={false}
      foot={at === "chief" ? <Composer placeholder="Ask your chief of staff" target="staff room · dust" onSend={noop} /> : <PanelNote>Done continues this conversation with your note.</PanelNote>}
    >
      {view === "terminal" ? (
        <Well label="dust">
          <pre>$ npm test{"\n"}212 passed</pre>
        </Well>
      ) : (
        <p>Two boards moved since this morning; #1443 asks you about the crumb.</p>
      )}
    </Panel>
  );
}

function FieldsDemo() {
  const [name, setName] = useState("build");
  const [on, setOn] = useState(true);
  const [range, setRange] = useState<"day" | "week" | "month">("week");
  return (
    <div class="design-line">
      <TextField label="Name" value={name} onInput={setName} />
      <TextField label="Note" value="" onInput={noop} placeholder="Why" hint="no secrets" />
      <Toggle label="Notify me" on={on} onChange={setOn} />
      <Choice label="Range" value={range} onChange={setRange} options={[{ id: "day", label: "Day" }, { id: "week", label: "Week" }, { id: "month", label: "Month" }]} />
    </div>
  );
}

function SheetDemo() {
  const [open, setOpen] = useState(false);
  const [side, setSide] = useState(false);
  return (
    <>
      <Button data-design-open="sheet" onClick={() => setOpen(true)}>
        Open a sheet
      </Button>
      <Button data-design-open="side-sheet" onClick={() => setSide(true)}>
        Open a side sheet
      </Button>
      {side ? (
        <Sheet side title="Running" onClose={() => setSide(false)}>
          <p>Every live process across your machines, the alerts first.</p>
        </Sheet>
      ) : null}
      {open ? (
        <Sheet title="Remove this board?" onClose={() => setOpen(false)}>
          <p>Its rows stay in the project's issues.</p>
          <div class="design-line">
            <Button kind="danger" onClick={() => setOpen(false)}>
              Remove
            </Button>
            <Button onClick={() => setOpen(false)}>Keep it</Button>
          </div>
        </Sheet>
      ) : null}
    </>
  );
}

// ---- Look: this page only

type Theme = "dark" | "light";

function paint(theme: Theme, s: Sliders) {
  const root = document.documentElement;
  for (const [k, v] of Object.entries(paintLook(theme === "dark", s))) root.style.setProperty(`--${k}`, v);
  root.style.colorScheme = theme;
  root.dataset.scheme = theme;
}

const SLIDER_WORDS: [keyof Sliders, string, string][] = [
  ["light", "light", "dark"],
  ["warmth", "cool", "warm"],
  ["depth", "flat", "deep"],
];

function LookSheet({ theme, sliders, set, close }: { theme: Theme; sliders: Sliders; set: (t: Theme, s: Sliders) => void; close: () => void }) {
  return (
    <Sheet title="Look" onClose={close}>
      <div class="design-look" data-design-look>
        <Choice label="Theme" value={theme} onChange={(t) => set(t, startAt(t === "dark"))} options={[{ id: "dark", label: "Dark" }, { id: "light", label: "Light" }]} />
        {SLIDER_WORDS.map(([k, lo, hi]) => (
          <label key={k} class="design-slider">
            <span class="m-meta">
              {lo} ↔ {hi}
            </span>
            <input type="range" min={0} max={1} step={0.05} value={sliders[k]} onInput={(e) => set(theme, { ...sliders, [k]: +e.currentTarget.value })} />
          </label>
        ))}
        <Button onClick={() => set(theme, startAt(theme === "dark"))}>Reset</Button>
      </div>
    </Sheet>
  );
}

const SECTIONS = [
  ["tokens", "Tokens"],
  ["surface", "Surface"],
  ["row", "Row"],
  ["need-row", "NeedRow"],
  ["well", "Well"],
  ["state-mark", "StateMark"],
  ["crumb", "Crumb"],
  ["kbd", "Kbd"],
  ["avatar", "Avatar"],
  ["panel", "Panel"],
  ["composer", "Composer"],
  ["tabs", "Tabs"],
  ["sheet", "Sheet"],
  ["button", "Button"],
  ["field", "Field"],
] as const;

export function DesignPage() {
  const own: Theme = palette().light ? "light" : "dark";
  const [theme, setTheme] = useState<Theme>(own);
  const [sliders, setSliders] = useState(() => startAt(own === "dark"));
  const [look, setLook] = useState(false);
  const set = (t: Theme, s: Sliders) => {
    paint(t, s);
    setTheme(t);
    setSliders(s);
  };

  // Leaving puts the person's own look back.
  useEffect(() => () => applyAppearance(), []);
  // `s` opens Look, as it will in the top bar.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "s" && !e.metaKey && !e.ctrlKey && !(e.target as HTMLElement).closest("input, textarea, [contenteditable]")) setLook(true);
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, []);
  // Sections are drawn after the page loads, so the browser can't scroll to
  // `#row` itself.
  useEffect(() => {
    const id = decodeURIComponent(location.hash.slice(1));
    if (id) document.getElementById(id)?.scrollIntoView();
  }, []);

  const S = (id: (typeof SECTIONS)[number][0]) => SECTIONS.find((s) => s[0] === id)![1];
  return (
    <div class="platform design-root" data-platform data-design-page>
      <main class="platform-main design-page">
        <header class="design-head">
          <h1>Design reference</h1>
          <span class="m-meta" data-design-scheme-shown={theme}>
            {theme}
          </span>
          <Button keys="s" onClick={() => setLook(true)} data-design-open="look">
            Look
          </Button>
        </header>
        <p class="design-use">The kit's fourteen parts, drawn by the real components. Build from these; the rules are in docs/design/language.md. Look changes this page only.</p>
        <nav class="design-nav" aria-label="Sections">
          {SECTIONS.map(([id, title]) => (
            <a key={id} href={`#${id}`}>
              {title}
            </a>
          ))}
        </nav>
        {look ? <LookSheet theme={theme} sliders={sliders} set={set} close={() => setLook(false)} /> : null}

        <Section id="tokens" title={S("tokens")} use="Every colour, face and size by its role, never a value. Look paints them.">
          <div class="design-swatches">
            {COLOURS.map((k) => (
              <div key={k} class="design-swatch" data-design-swatch={k} style={{ background: `var(${k})` }}>
                <code>{k}</code>
              </div>
            ))}
          </div>
          {SIZES.map((k) => (
            <div key={k} data-design-type={k} style={{ fontSize: `var(${k})` }}>
              {k}: Crumb at 900px
            </div>
          ))}
        </Section>
        <Section id="surface" title={S("surface")} use="The canvas, then a section, then cards on it; paper for a Doc; an overlay floats.">
          <Surface kind="section">
            <div class="design-line">
              <Surface>A card</Surface>
              <Surface current>The open card</Surface>
              <Surface kind="paper">Paper</Surface>
              <Surface kind="overlay">An overlay</Surface>
            </div>
          </Surface>
        </Section>
        <Section id="row" title={S("row")} use="One flat line: mark, kind, title, who and meta, age. Never a card of its own.">
          <Surface kind="section">
            <Row mark="working" label="board" title="Product improvements" meta={["claude@dust", "4m"]} onClick={noop} />
            <Row kind="selected" mark="idle" label="doc" title="Release cadence" meta={["aaron", "2d"]} onClick={noop} />
            <Row kind="need" mark="asks" label="ask" title="Crumb at 900px?" meta={[<span class="k-needs">asks you</span>, "7m"]} onClick={noop} />
          </Surface>
        </Section>
        <Section id="need-row" title={S("need-row")} use="An agent asks or waits on you, answered in the row. Amber is the mark, the word and a thin edge, never a fill. Drawn here at a third of the screen.">
          <NeedRows />
        </Section>
        <Section id="well" title={S("well")} use="Inset, flat and mono, its label in the frame: terminals, agent panes, something to copy.">
          <Well label="how">
            <pre>{"cd server\njust migrate"}</pre>
          </Well>
        </Section>
        <Section id="state-mark" title={S("state-mark")} use="A state is a mark and always its word.">
          <div class="design-line">
            {(Object.keys(MARKS) as State[]).map((s) => (
              <StateMark key={s} state={s} />
            ))}
          </div>
        </Section>
        <Section id="crumb" title={S("crumb")} use="A path you can climb; the first step is the project.">
          <Crumb steps={[{ label: "Arugula", go: noop }, { label: "Boards", go: noop }, { label: "Product improvements", go: noop }, { label: "#1443", id: true }]} />
        </Section>
        <Section id="kbd" title={S("kbd")} use="Every action has a key, shown as a key.">
          <div class="design-line">
            <Kbd keys="⌘K" /> <Kbd keys="g o" /> <Kbd keys="esc" />
          </div>
        </Section>
        <Section id="avatar" title={S("avatar")} use="A person is initials on a disc; an agent is the ✻ mark.">
          <div class="design-line">
            <Avatar name="Aaron Biller" /> <Avatar name="Jake" /> <Avatar name="claude@dust" agent />
          </div>
        </Section>
        <Section id="panel" title={S("panel")} use="The agent panel on the right: tabs, who it is, the transcript, the composer.">
          <PanelDemo />
        </Section>
        <Section id="composer" title={S("composer")} use="Where you write to an agent or a room. It glows while you're in it; ↵ sends.">
          <Composer placeholder="Message #design" target="to the room" onSend={noop} />
        </Section>
        <Section id="tabs" title={S("tabs")} use="An icon, a title, a count and a close; the arrows move between them.">
          <TabsDemo />
        </Section>
        <Section id="sheet" title={S("sheet")} use="The one overlay: Go to…, the keys, Look, a menu, a confirm. Esc closes it.">
          <SheetDemo />
        </Section>
        <Section id="button" title={S("button")} use="One primary to a place; plain; danger. A key that does the same shows inside it.">
          <div class="design-line">
            <Button kind="primary" keys="y">
              Allow
            </Button>
            <Button keys="x">Deny</Button>
            <Button kind="danger">Remove</Button>
            <Button disabled>Disabled</Button>
          </div>
        </Section>
        <Section id="field" title={S("field")} use="A text box, a toggle that takes effect now, a choice of one of a few. Each has its label.">
          <FieldsDemo />
        </Section>
      </main>
    </div>
  );
}
