// The design reference (#693): every shared piece of the UI, drawn by the
// real components, for people and agents to check new UI against. It is the
// written language (docs/design/language.md) made visible, in the same order,
// each section with an anchor (`/design#chips`), what the piece is for and
// where it is imported from.
//
// It needs no one signed in and no project, and imports only shared/ and
// platform/. Nothing here copies a piece's markup. And what can be read from
// the source is read from it, so a new piece shows up without editing this
// page: the icons are the exports of icons.tsx, the chip and card tones are
// the `Tone` type's, the palette is what `legible()` returns, and the type,
// spacing, corner, shadow and icon sizes are the custom properties
// style.css defines. What is written here by hand: each section's words (what
// it is for, where it is imported from), the job of each type size, and the
// sample props of the pieces that need data.

import type { ComponentChildren, FunctionComponent } from "preact";
import { useEffect, useMemo, useState } from "preact/hooks";
import { apply as applyAppearance, legible, palette, PALETTES } from "../../shared/appearance";
import { paint as paintLook } from "../../shared/kit/look";
import { Bar, BarTitle, BarWhere } from "../../shared/ui/bar";
import { EmptyLine, EmptyState, BlockError, BlockLoading } from "../../shared/ui/block-states";
import { Card } from "../../shared/ui/card";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { Guard } from "../../shared/ui/guard";
import * as iconModule from "../../shared/ui/icons";
import { CloseIcon, IconButton, LinkIcon, PaneTypeIcon, PlusIcon, SearchIcon } from "../../shared/ui/icons";
import { ListPage, ListPreview } from "../../shared/ui/list-page";
import { menuBelow, openMenu, type MenuItem } from "../../shared/ui/menu";
import { Modal } from "../../shared/ui/modal";
import { PageHeader } from "../../shared/ui/page-header";
import { TimeRange, Weekdays } from "../../shared/ui/schedule";
import { SearchField } from "../../shared/ui/search-field";
import { SectionHead } from "../../shared/ui/section-head";
import { SideSheet } from "../../shared/ui/side-sheet";
import { Chip, Count, HintChip, ObjectChip, StateChip, type Tone } from "../../shared/ui/state-chip";
import chipSource from "../../shared/ui/state-chip.tsx?raw";
import "../design.css";

// ---- read from the source

/** The chip's and the card's tones: the members of `Tone` in state-chip.tsx,
 * read from the file itself (a type has no value to iterate). */
const TONES = [...(/export type Tone =([^;]+);/.exec(chipSource)?.[1] ?? "").matchAll(/"([a-z]+)"/g)].map((m) => m[1] as Tone);

/** The custom properties a stylesheet defines whose name matches `re`, in any
 * rule (`:root`, a theme variant, a media query). */
function tokens(re: RegExp): string[] {
  const found = new Set<string>();
  const walk = (rules: CSSRuleList) => {
    for (const rule of Array.from(rules)) {
      if (rule instanceof CSSStyleRule) {
        for (const name of Array.from(rule.style)) if (name.startsWith("--") && re.test(name)) found.add(name);
      } else if ("cssRules" in rule) walk((rule as CSSGroupingRule).cssRules);
    }
  };
  for (const sheet of Array.from(document.styleSheets)) {
    try {
      walk(sheet.cssRules);
    } catch {
      // another origin's sheet: not ours to read
    }
  }
  return [...found];
}

/** What a token is now, under the page's palette. */
const valueOf = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

const numeric = (a: string, b: string) => parseFloat(valueOf(a)) - parseFloat(valueOf(b)) || a.localeCompare(b);

/** Every export of icons.tsx is an icon but these two, which draw one. */
const NOT_ICONS = new Set(["Svg", "IconButton"]);

/** The icons that need a prop to draw anything, with one to draw them by. */
const NEEDS: Record<string, Record<string, unknown>> = {
  FocusIcon: { on: true },
  SourceGlyph: { kind: "github" },
  CalloutIcon: { alert: "note" },
  PaneTypeIcon: { type: "terminal" },
  KindIcon: { kind: "shell" },
};

/** The type sizes' jobs, from language.md. A size without one says so here. */
const TYPE_JOB: Record<string, string> = {
  xs: "caps label, chips, tags, ids, times, kbd",
  sm: "meta, secondary lines, buttons",
  md: "body: row titles, messages, form text",
  lg: "a title: dialogs, side sheets, empty states",
  xl: "a section h2 (as written, not caps), stat numbers",
  "2xl": "a page h1",
  doc: "the Doc's own title",
};

// ---- the page's pieces

/** One section: its anchor, what the piece is for, and where it is imported from. */
function Section({ id, title, use, from, children }: { id: string; title: string; use: string; from: string; children: ComponentChildren }) {
  return (
    <section class="design-section" id={id} data-design-section={id} aria-labelledby={`${id}-title`}>
      <SectionHead title={<span id={`${id}-title`}>{title}</span>} rule />
      <p class="design-use" data-design-use>
        {use}
      </p>
      <p class="design-from">
        <code data-design-import>{from}</code>
      </p>
      {children}
    </section>
  );
}

/** A labelled sample: the label says what it is. */
function Sample({ label, children, wide }: { label: string; children: ComponentChildren; wide?: boolean }) {
  return (
    <div class={wide ? "design-sample wide" : "design-sample"}>
      <div class="design-label">{label}</div>
      <div class="design-show">{children}</div>
    </div>
  );
}

const noop = () => {};

// ---- sections

function TypeScale() {
  // Today's steps; kit/tokens.css's role names for them (--text-label …)
  // join when the kit rebuilds this page (design lift, phase 2).
  const names = useMemo(() => tokens(/^--text-(xs|sm|md|lg|xl|2xl|doc)$/).sort(numeric), []);
  return (
    <div class="design-type">
      {names.map((name) => {
        const step = name.slice("--text-".length);
        const job = TYPE_JOB[step];
        return (
          <div key={name} class="design-type-row" data-design-type={step}>
            <code class="design-token">{name}</code>
            <span class="design-type-size">{valueOf(name)}</span>
            <span class="design-type-sample" style={{ fontSize: `var(${name})`, lineHeight: `var(--lh-${step})` }}>
              Sessions keep running
            </span>
            <span class="design-type-job">{job ?? "(add its job to TYPE_JOB in pages/design.tsx)"}</span>
          </div>
        );
      })}
    </div>
  );
}

function Colour({ scheme }: { scheme: string }) {
  // The palette's tokens, as legible() paints them: its keys are the list.
  const names = useMemo(() => Object.keys(legible(PALETTES[0])), []);
  return (
    <div class="design-swatches" data-design-scheme-shown={scheme}>
      {names.map((k) => (
        <div key={k} class="design-swatch" data-design-swatch={k}>
          <span class="design-swatch-fill" style={{ background: `var(--${k})` }} />
          <code class="design-token">--{k}</code>
          <span class="design-swatch-value">{valueOf(`--${k}`)}</span>
        </div>
      ))}
    </div>
  );
}

function Buttons() {
  const [size, setSize] = useState("page");
  const [filter, setFilter] = useState("open");
  return (
    <>
      <Sample label="page buttons, 32 (40 on a touch screen)">
        <button type="button" class="primary">
          Primary
        </button>
        <button type="button" class="secondary">
          Secondary
        </button>
        <button type="button" class="primary danger">
          Danger
        </button>
        <button type="button" class="secondary danger">
          Danger, quiet
        </button>
        <button type="button" class="secondary small">
          Small
        </button>
        <button type="button" class="link">
          Link
        </button>
      </Sample>
      <Sample label="bar buttons, 24 (the ghost)">
        <button type="button" class="bar-btn">
          Ghost
        </button>
        <IconButton label="Search" class="bar-btn sq">
          <SearchIcon size="sm" />
        </IconButton>
        <button type="button" class="secondary small">
          <PlusIcon size="xs" /> Add row
        </button>
      </Sample>
      <Sample label="disabled">
        <button type="button" class="primary" disabled>
          Primary
        </button>
        <button type="button" class="secondary" disabled>
          Secondary
        </button>
        <button type="button" class="primary danger" disabled>
          Danger
        </button>
        <button type="button" class="bar-btn" disabled>
          Ghost
        </button>
        <button type="button" class="link" disabled>
          Link
        </button>
      </Sample>
      <Sample label="segmented control: a filter (aria-pressed), small, default and touch">
        {(["small", "", "touch"] as const).map((cls) => (
          <div key={cls} class={`seg ${cls}`.trim()} role="group" aria-label={`Filter, ${cls || "default"}`}>
            {["open", "closed", "all"].map((f) => (
              <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)}>
                {f[0].toUpperCase() + f.slice(1)}
              </button>
            ))}
          </div>
        ))}
      </Sample>
      <Sample label="segmented control: tabs (aria-selected)">
        <div class="seg" role="tablist" aria-label="Size">
          {["page", "bar", "touch"].map((s) => (
            <button key={s} type="button" role="tab" aria-selected={size === s} onClick={() => setSize(s)}>
              {s[0].toUpperCase() + s.slice(1)}
            </button>
          ))}
        </div>
      </Sample>
    </>
  );
}

/** One state of a control, named under it. */
function State({ name, children }: { name: string; children: ComponentChildren }) {
  return (
    <div class="design-state" data-design-field-state={name}>
      {children}
      <span class="design-state-name">{name}</span>
    </div>
  );
}

/** A box or a dot with its words, as a form has it. */
function Choice({ children }: { children: ComponentChildren }) {
  return <label class="design-choice">{children}</label>;
}

/** Marks a checkbox as some of a list: only code can. */
const indeterminate = (el: HTMLInputElement | null) => {
  if (el) el.indeterminate = true;
};

function Fields() {
  const [query, setQuery] = useState("");
  const [found, setFound] = useState("installer");
  const [days, setDays] = useState([1, 2, 3, 4, 5]);
  const [range, setRange] = useState({ from: "09:00", to: "17:30" });
  const [night, setNight] = useState({ from: "22:00", to: "07:00" });
  const [seg, setSeg] = useState("week");
  const [sw, setSw] = useState(true);
  return (
    <>
      <Sample label="focus" wide>
        <span class="dim">Click or Tab into any field below: its line goes accent, with a halo.</span>
      </Sample>
      <Sample label="text: 32 on a page (40 on touch, 16px text on a phone)" wide>
        <State name="empty">
          <input aria-label="Name, empty" placeholder="Name" />
        </State>
        <State name="filled">
          <input aria-label="Name, filled" value="build" />
        </State>
        <State name="invalid">
          <input aria-label="Name, invalid" aria-invalid="true" value="build/" />
        </State>
        <State name="disabled">
          <input aria-label="Name, disabled" disabled value="build" />
        </State>
        <State name="small: 24, in a bar">
          <input class="small" aria-label="Name, small" placeholder="Name" />
        </State>
      </Sample>
      <Sample label="search: SearchField; the page's main one answers / and shows the key" wide>
        <State name="empty, with /">
          <SearchField label="Search the samples" value={query} onValue={setQuery} slash />
        </State>
        <State name="filled: Esc clears it">
          <SearchField label="Search, filled" value={found} onValue={setFound} />
        </State>
        <State name="small: 24, in a bar">
          <SearchField label="Search, small" value="" onValue={noop} small />
        </State>
      </Sample>
      <Sample label="select: the kit's caret on every system" wide>
        <State name="empty">
          <select aria-label="Machine, empty">
            <option value="">Pick a machine</option>
            <option>mac-mini</option>
          </select>
        </State>
        <State name="filled">
          <select aria-label="Machine, filled">
            <option>mac-mini</option>
            <option>build-vm</option>
          </select>
        </State>
        <State name="invalid">
          <select aria-label="Machine, invalid" aria-invalid="true">
            <option value="">Pick a machine</option>
          </select>
        </State>
        <State name="disabled">
          <select aria-label="Machine, disabled" disabled>
            <option>mac-mini</option>
          </select>
        </State>
      </Sample>
      <Sample label="textarea: grows by hand, prose line-height" wide>
        <State name="empty">
          <textarea aria-label="Note, empty" placeholder="Add a note" rows={2} />
        </State>
        <State name="filled">
          <textarea aria-label="Note, filled" rows={2} value="Ship it after the screenshots." />
        </State>
        <State name="invalid">
          <textarea aria-label="Note, invalid" aria-invalid="true" rows={2} value="" />
        </State>
        <State name="disabled">
          <textarea aria-label="Note, disabled" disabled rows={2} value="Read only" />
        </State>
      </Sample>
      <Sample label="checkbox, a box: pick, then press a button to commit" wide>
        <State name="off">
          <Choice>
            <input type="checkbox" /> Open
          </Choice>
        </State>
        <State name="on">
          <Choice>
            <input type="checkbox" checked /> Closed
          </Choice>
        </State>
        <State name="indeterminate: some of a list">
          <Choice>
            <input type="checkbox" ref={indeterminate} /> All
          </Choice>
        </State>
        <State name="invalid">
          <Choice>
            <input type="checkbox" aria-invalid="true" /> I agree
          </Choice>
        </State>
        <State name="disabled">
          <Choice>
            <input type="checkbox" disabled /> Off
          </Choice>
          <Choice>
            <input type="checkbox" disabled checked /> On
          </Choice>
        </State>
      </Sample>
      <Sample label="radio, a dot: one of a few that each need words" wide>
        <State name="off, on">
          <Choice>
            <input type="radio" name="design-radio" /> Everyone in the project
          </Choice>
          <Choice>
            <input type="radio" name="design-radio" checked /> Only me
          </Choice>
        </State>
        <State name="invalid">
          <Choice>
            <input type="radio" name="design-radio-bad" aria-invalid="true" /> Pick one
          </Choice>
        </State>
        <State name="disabled">
          <Choice>
            <input type="radio" name="design-radio-off" disabled /> Off
          </Choice>
          <Choice>
            <input type="radio" name="design-radio-off" disabled checked /> On
          </Choice>
        </State>
      </Sample>
      <Sample label="switch: takes effect now, no button" wide>
        <State name="off">
          <input type="checkbox" role="switch" class="sw-switch" aria-label="Off" />
        </State>
        <State name="on">
          <input type="checkbox" role="switch" class="sw-switch" aria-label="On" checked={sw} onChange={(e) => setSw(e.currentTarget.checked)} />
        </State>
        <State name="partial">
          <input type="checkbox" role="switch" class="sw-switch partial" aria-label="Partly on" title="Some of it: read, not write" />
        </State>
        <State name="busy">
          <input type="checkbox" role="switch" class="sw-switch" aria-label="Being turned" aria-busy="true" checked />
        </State>
        <State name="disabled">
          <input type="checkbox" role="switch" class="sw-switch" aria-label="Off, disabled" disabled />
          <input type="checkbox" role="switch" class="sw-switch" aria-label="On, disabled" disabled checked />
        </State>
      </Sample>
      <Sample label="segments: one of two to four, short (Buttons has the sizes)" wide>
        <State name="one on">
          <div class="seg" role="group" aria-label="Range">
            {["day", "week", "month"].map((v) => (
              <button key={v} type="button" aria-pressed={seg === v} onClick={() => setSeg(v)}>
                {v[0].toUpperCase() + v.slice(1)}
              </button>
            ))}
          </div>
        </State>
        <State name="disabled">
          <div class="seg" role="group" aria-label="Range, disabled">
            {["day", "week", "month"].map((v) => (
              <button key={v} type="button" aria-pressed={v === "week"} disabled>
                {v[0].toUpperCase() + v.slice(1)}
              </button>
            ))}
          </div>
        </State>
      </Sample>
      <Sample label="weekday chips: Weekdays, any of the seven days" wide>
        <State name="none">
          <Weekdays label="Days, none" value={[]} onChange={noop} />
        </State>
        <State name="some">
          <Weekdays label="Days" value={days} onChange={setDays} />
        </State>
        <State name="disabled">
          <Weekdays label="Days, disabled" value={[6, 7]} onChange={noop} disabled />
        </State>
      </Sample>
      <Sample label="time range: TimeRange, from and to in one field" wide>
        <State name="empty">
          <TimeRange label="Empty" from="" to="" onChange={noop} />
        </State>
        <State name="filled">
          <TimeRange label="Day" from={range.from} to={range.to} onChange={setRange} />
        </State>
        <State name="overnight">
          <TimeRange label="Night" from={night.from} to={night.to} onChange={setNight} />
        </State>
        <State name="disabled">
          <TimeRange label="Disabled" from="09:00" to="17:00" onChange={noop} disabled />
        </State>
      </Sample>
    </>
  );
}

function Headers() {
  const more: MenuItem[] = [
    { label: "Rename…", run: noop },
    { label: "Archive", run: noop },
  ];
  return (
    <>
      <Sample label="the bar: 34 high, never wraps; what doesn't fit goes into the menu" wide>
        <div class="design-box">
          <Bar
            lead={<PaneTypeIcon type="terminal" />}
            title={<BarTitle>build</BarTitle>}
            where={<BarWhere>~/arugula</BarWhere>}
            status={<StateChip tone="ok">running</StateChip>}
            fold={[
              { key: "link", node: <IconButton label="Copy link" class="bar-btn sq"><LinkIcon size="sm" /></IconButton>, item: { label: "Copy link", run: noop } },
              { key: "search", node: <IconButton label="Search" class="bar-btn sq"><SearchIcon size="sm" /></IconButton>, item: { label: "Search", run: noop } },
            ]}
            actions={
              <IconButton label="Close pane" class="bar-btn sq">
                <CloseIcon size="sm" />
              </IconButton>
            }
          />
          <Bar sub title={<BarTitle>A block's own bar, under the pane's</BarTitle>} />
        </div>
      </Sample>
      <Sample label="the same bar in a narrow column: the least important go into the menu" wide>
        <div class="design-box design-narrow">
          <Bar
            lead={<PaneTypeIcon type="agent" />}
            title={<BarTitle>fix the installer</BarTitle>}
            where={<BarWhere>~/arugula</BarWhere>}
            status={<StateChip tone="warn">asks</StateChip>}
            foldBelow={[320, 280]}
            fold={[
              { key: "link", node: <IconButton label="Copy link" class="bar-btn sq"><LinkIcon size="sm" /></IconButton>, item: { label: "Copy link", run: noop } },
              { key: "search", node: <IconButton label="Search" class="bar-btn sq"><SearchIcon size="sm" /></IconButton>, item: { label: "Search", run: noop } },
            ]}
            actions={
              <IconButton label="Close pane" class="bar-btn sq">
                <CloseIcon size="sm" />
              </IconButton>
            }
          />
        </div>
      </Sample>
      <Sample label="the page header: back, title, state, actions, sub line, stats" wide>
        <div class="design-box">
          <PageHeader
            back={{ href: "#headers", label: "Projects" }}
            title="Release plan"
            named
            state={<StateChip tone="ok">Open</StateChip>}
            actions={
              <>
                <button type="button" class="secondary small">
                  Edit
                </button>
                <button type="button" class="primary small">
                  Run
                </button>
              </>
            }
            more={more}
            sub="Updated two hours ago, on this machine"
            stats={
              <div class="sw-stats">
                <span>
                  <b>3</b> issues
                </span>
              </div>
            }
          />
        </div>
      </Sample>
    </>
  );
}

type Row = { id: string; title: string; detail: string; age: string; tone?: "live" | "bad" | "warn" };
const ROWS: Row[] = [
  { id: "a", title: "Fix the installer", detail: "Waiting on review", age: "2h", tone: "live" },
  { id: "b", title: "Tidy the changelog", detail: "Nothing to do yet", age: "1d" },
  { id: "c", title: "Update the screenshots", detail: "Failed on the last run", age: "3d", tone: "bad" },
];

function Rows() {
  const [query, setQuery] = useState("");
  const [card, setCard] = useState("b");
  const shown = ROWS.filter((r) => r.title.toLowerCase().includes(query.toLowerCase()));
  return (
    <>
      <Sample label="flat rows, in a list page: click or use the arrow keys to select one" wide>
        <div class="design-list" data-design-list>
          <ListPage<Row>
            label="Issues"
            header={<h3 class="design-list-head">Issues</h3>}
            searchLabel="Search the issues"
            query={query}
            setQuery={setQuery}
            groups={[{ title: "Open", items: shown }]}
            keyOf={(r) => r.id}
            row={(r) => ({ title: r.title, detail: r.detail, age: r.age, tone: r.tone })}
            preview={(r) => (
              <ListPreview title={r.title} facts={r.detail}>
                <p>The selected row keeps its tint under the pointer.</p>
              </ListPreview>
            )}
            onOpen={noop}
            splitKey="arugula.design-reference-split"
          />
        </div>
      </Sample>
      <Sample label="card rows: the selected one has the tint and the edge, whatever its tone" wide>
        <div class="card-list">
          {ROWS.map((r) => (
            <Card key={r.id} tone={r.tone === "live" ? "accent" : r.tone === "bad" ? "danger" : undefined} selected={card === r.id} data-design-card-row={r.id} onClick={() => setCard(r.id)}>
              <strong>{r.title}</strong> <span class="dim">{r.detail}</span>
            </Card>
          ))}
        </div>
      </Sample>
    </>
  );
}

function Chips() {
  return (
    <>
      <Sample label="status: StateChip, a dot and its word, in a tone" wide>
        {TONES.map((t) => (
          <StateChip key={t} tone={t} data-design-chip={`status-${t}`}>
            {t}
          </StateChip>
        ))}
      </Sample>
      <Sample label="tag: Chip kind=tag, a label" wide>
        {TONES.map((t) => (
          <Chip key={t} kind="tag" tone={t} data-design-chip={`tag-${t}`}>
            {t}
          </Chip>
        ))}
      </Sample>
      <Sample label="count: Count, neutral, for you, a mention" wide>
        <Count data-design-chip="count">3</Count>
        <Count for="you" data-design-chip="count-you">
          3
        </Count>
        <Count for="mention" data-design-chip="count-mention">
          3
        </Count>
      </Sample>
      <Sample label="object: ObjectChip, an id, its title and a fact in a tone; dashed for proof, a pair as two cells" wide>
        {TONES.map((t) => (
          <ObjectChip key={t} id="#693" fact={{ tone: t, text: t }} data-design-chip={`object-${t}`}>
            Reference page
          </ObjectChip>
        ))}
        <ObjectChip id="#693" proof data-design-chip="object-proof">
          Proof
        </ObjectChip>
        <ObjectChip id="#693" pair fact={{ tone: "ok", text: "merged" }} data-design-chip="object-pair">
          Pair
        </ObjectChip>
        <ObjectChip id="#693" onClick={noop} onRemove={noop} removeLabel="Remove #693" data-design-chip="object-button">
          Removable
        </ObjectChip>
      </Sample>
      <Sample label="hint: HintChip, a tip about a pane" wide>
        <HintChip id="design" copy="Drag a tab onto a pane to split it." action="Try it" onAct={noop} onDismiss={noop} />
      </Sample>
    </>
  );
}

function Cards() {
  return (
    <>
      <Sample label="tone: a bar, an edge and a fill; none, or null for a toned card with no state" wide>
        <div class="card-grid">
          <Card data-design-card="plain">
            <strong>plain</strong>
          </Card>
          <Card tone={null} data-design-card="null">
            <strong>null</strong>
          </Card>
          {TONES.map((t) => (
            <Card key={t} tone={t} data-design-card={t}>
              <strong>{t}</strong>
            </Card>
          ))}
        </div>
      </Sample>
      <Sample label="dense: one line" wide>
        <div class="card-list">
          <Card dense tone="ok" data-design-card="dense">
            Fix the installer
          </Card>
          <Card dense selected data-design-card="dense-selected">
            Selected, dense
          </Card>
        </div>
      </Sample>
      <Sample label="bar: a Bar as its head" wide>
        <div class="card-grid">
          <Card tone="info" head={<Bar sub title={<BarTitle>build</BarTitle>} status={<StateChip tone="info">running</StateChip>} />} data-design-card="bar">
            The body keeps the card's padding.
          </Card>
        </div>
      </Sample>
    </>
  );
}

function Icons() {
  // Every export of icons.tsx but the two wrappers: a new icon appears here by being added there.
  const icons = useMemo(
    () => Object.entries(iconModule).filter(([name, v]) => typeof v === "function" && !NOT_ICONS.has(name)) as [string, FunctionComponent<Record<string, unknown>>][],
    [],
  );
  return (
    <>
      <div class="design-icons" data-design-icons={icons.length}>
        {icons.map(([name, Icon]) => (
          <div key={name} class="design-icon" data-design-icon={name}>
            {/* An icon that needs props says so, so the page can't break on one. */}
            <Guard failed={() => <span class="design-icon-bad">needs props: add it to NEEDS in pages/design.tsx</span>}>
              <Icon size="md" {...NEEDS[name]} />
            </Guard>
            <code class="design-token">{name}</code>
          </div>
        ))}
      </div>
      <Sample label="sizes: xs 12, sm 14, md 16, lg 18">
        {(["xs", "sm", "md", "lg"] as const).map((size) => (
          <span key={size} class="design-size" data-design-icon-size={size}>
            <SearchIcon size={size} /> {size}
          </span>
        ))}
      </Sample>
      <Sample label="an icon alone is a button with a name: IconButton">
        <IconButton label="Search" class="bar-btn sq" data-design-icon-button>
          <SearchIcon size="sm" />
        </IconButton>
        <IconButton label="Search" class="secondary sq small">
          <SearchIcon size="sm" />
        </IconButton>
      </Sample>
    </>
  );
}

function Dialogs() {
  const [open, setOpen] = useState<"modal" | "confirm" | "sheet" | null>(null);
  const close = () => setOpen(null);
  return (
    <>
      <Sample label="open one: Escape or the backdrop closes it, and the focus goes back here">
        <button type="button" class="secondary" data-design-open="modal" onClick={() => setOpen("modal")}>
          Dialog
        </button>
        <button type="button" class="secondary" data-design-open="confirm" onClick={() => setOpen("confirm")}>
          Confirm remove
        </button>
        <button type="button" class="secondary" data-design-open="sheet" onClick={() => setOpen("sheet")}>
          Side sheet
        </button>
      </Sample>
      {open === "modal" && (
        <Modal
          title="Rename the pane"
          close={close}
          root={{ "data-design-dialog": "modal" }}
          buttons={
            <>
              <button type="button" class="secondary" onClick={close}>
                Cancel
              </button>
              <button type="button" class="primary" onClick={close}>
                Rename
              </button>
            </>
          }
        >
          <input class="field" aria-label="Name" data-autofocus />
        </Modal>
      )}
      {open === "confirm" && (
        <ConfirmRemove title="Close this pane?" go={close} cancel={close}>
          <p>What runs in it stops.</p>
        </ConfirmRemove>
      )}
      {open === "sheet" && (
        <SideSheet
          title="Fix the installer"
          subtitle="Next, on the Launch board"
          close={close}
          root={{ "data-design-dialog": "sheet" }}
          foot={
            <>
              <button type="button" class="secondary" onClick={close}>
                Cancel
              </button>
              <button type="button" class="primary" onClick={close}>
                Save
              </button>
            </>
          }
        >
          <p>A panel at the right edge that shows one thing beside the page. At 600px and under it is a bottom sheet.</p>
        </SideSheet>
      )}
    </>
  );
}

function Menus() {
  const items: MenuItem[] = [
    { header: "Pane" },
    { label: "Rename…", run: noop, shortcut: "F2" },
    { label: "Move to", submenu: [{ label: "Next tab", run: noop }, { label: "A new tab", run: noop }] },
    "separator",
    { header: "Show" },
    { label: "Open", checked: true, multi: true, run: noop },
    { label: "Closed", checked: false, multi: true, run: noop },
    "separator",
    { label: "Close pane", danger: true, run: noop },
  ];
  const labels: MenuItem[] = ["design-system", "spike", "enhancement", "docs", "bug", "chat", "boards", "phone", "platform", "docs-site", "good first issue", "needs-design", "release", "a11y"].map((label) => ({
    label,
    detail: String(label.length),
    checked: false,
    multi: true,
    run: noop,
  }));
  return (
    <>
    <Sample label="a menu with a submenu, a separator, a facet and a danger item: Escape closes it">
      <button type="button" class="secondary" aria-haspopup="menu" data-design-open="menu" onClick={(e) => openMenu(e, items, e.currentTarget)}>
        Open the menu
      </button>
    </Sample>
    <Sample label="13 rows or more: the menu starts with a filter row; typing narrows the rows, Escape clears it, then closes">
      <button type="button" class="secondary" aria-haspopup="menu" data-design-open="long-menu" onClick={(e) => openMenu(menuBelow(e.currentTarget), labels, { from: e.currentTarget, label: "Labels" })}>
        Open a long menu
      </button>
    </Sample>
    </>
  );
}

function States() {
  return (
    <>
      <Sample label="empty, on a page: a statement, a why and at most one primary" wide>
        <EmptyState where="page" title="No rows yet." why="Rows are the issues on this board, from the project's tracker." action={<button type="button" class="primary">Add a row</button>} data-design-state="empty-page" />
      </Sample>
      <Sample label="empty, in a pane: the same card, centred" wide>
        <div class="design-pane">
          <EmptyState where="pane" title="Nothing is running." why="Start a terminal or an agent from the plus." data-design-state="empty-pane" />
        </div>
      </Sample>
      <Sample label="no match: a dim line, with Clear" wide>
        <EmptyLine clear={noop} data-design-state="empty-line">
          No issue matches.
        </EmptyLine>
      </Sample>
      <Sample label="loading: says nothing for the first 300 ms" wide>
        <BlockLoading what="the issues" inline data-design-state="loading" />
      </Sample>
      <Sample label="error: Can't…, the detail under it, and Try again" wide>
        <BlockError inline title="Can't read the issue" detail="Not found: 404" retry={noop} data-design-state="error" />
      </Sample>
    </>
  );
}

function Spacing({ scheme }: { scheme: string }) {
  const spaces = useMemo(() => tokens(/^--space-\d+$/).sort(numeric), []);
  const radii = useMemo(() => tokens(/^--[a-z]+-radius$/).sort(), []);
  const shadows = useMemo(() => tokens(/^--shadow-(?!k$)[a-z]+$/).sort(), []);
  const sizes = useMemo(() => tokens(/^--icon-[a-z]+$/).sort(numeric), []);
  return (
    <>
      <Sample label="space: use the scale">
        <div class="design-scale" data-design-scheme-shown={scheme}>
          {spaces.map((name) => (
            <div key={name} class="design-scale-row" data-design-space={name}>
              <code class="design-token">{name}</code>
              <span class="design-space-bar" style={{ width: `var(${name})` }} />
              <span class="design-swatch-value">{valueOf(name)}</span>
            </div>
          ))}
        </div>
      </Sample>
      <Sample label="corners: the radius tokens, never a raw px">
        {radii.map((name) => (
          <div key={name} class="design-corner" data-design-radius={name} style={{ borderRadius: `var(${name})` }}>
            <code class="design-token">{name}</code>
            <span class="design-swatch-value">{valueOf(name)}</span>
          </div>
        ))}
      </Sample>
      <Sample label="shadows">
        {shadows.map((name) => (
          <div key={name} class="design-shadow" data-design-shadow={name} style={{ boxShadow: `var(${name})` }}>
            <code class="design-token">{name}</code>
          </div>
        ))}
      </Sample>
      <Sample label="icon sizes">
        {sizes.map((name) => (
          <span key={name} class="design-size" data-design-icon-token={name}>
            <code class="design-token">{name}</code> <span class="design-swatch-value">{valueOf(name)}</span>
          </span>
        ))}
      </Sample>
    </>
  );
}

// ---- the page

const SECTIONS = [
  ["type", "Type"],
  ["colour", "Colour"],
  ["buttons", "Buttons"],
  ["fields", "Fields"],
  ["headers", "Headers"],
  ["rows", "Rows and the selected state"],
  ["chips", "Chips"],
  ["cards", "Cards"],
  ["icons", "Icons"],
  ["dialogs", "Dialogs and side sheets"],
  ["menus", "Menus"],
  ["states", "Empty and error states"],
  ["spacing", "Spacing and shadows"],
] as const;

type Scheme = "slate" | "latte";

/** Paints the document in `id`'s palette, as `apply()` does, but keeps no
 * setting: the person's own comes back with `apply()`. */
function paint(id: Scheme) {
  const p = PALETTES.find((x) => x.id === id)!;
  const root = document.documentElement;
  for (const [k, v] of Object.entries(paintLook(p.ui, !p.light))) root.style.setProperty(`--${k}`, v);
  root.style.colorScheme = p.light ? "light" : "dark";
  root.dataset.scheme = p.light ? "light" : "dark";
  root.dataset.palette = p.id;
}

export function DesignPage() {
  const own = palette().id;
  // Nothing is pressed until a palette is chosen here, unless the person's is one of the two.
  const [scheme, setScheme] = useState<Scheme | null>(own === "slate" || own === "latte" ? own : null);

  const pick = (id: Scheme) => {
    paint(id);
    setScheme(id);
  };

  // Leaving puts the person's own palette back.
  useEffect(() => () => applyAppearance(), []);

  // Sections are drawn after the page loads, so the browser can't scroll to
  // `#chips` itself (a click on a link in the page can: its target is there).
  useEffect(() => {
    const id = decodeURIComponent(location.hash.slice(1));
    if (id) document.getElementById(id)?.scrollIntoView();
  }, []);

  const shown = scheme ?? own;
  return (
    <div class="platform design-root" data-platform data-design-page>
      <main class="platform-main design-page">
        <PageHeader
          title="Design reference"
          state={<Chip kind="tag">{shown}</Chip>}
          actions={
            <div class="seg" role="group" aria-label="Palette for this page">
              {(["slate", "latte"] as const).map((id) => (
                <button key={id} type="button" aria-pressed={scheme === id} data-design-scheme={id} onClick={() => pick(id)}>
                  {id[0].toUpperCase() + id.slice(1)}
                </button>
              ))}
            </div>
          }
          sub="The shared pieces of the UI, drawn by the real components. Check new UI against this page; the rules are in docs/design/language.md. The palette switch changes this page only."
        />
        <nav class="design-nav" aria-label="Sections">
          {SECTIONS.map(([id, title]) => (
            <a key={id} href={`#${id}`}>
              {title}
            </a>
          ))}
        </nav>
        <Section id="type" title="Type" use="Every size of text is a step on this scale; name the step, never a number." from="shared/style.css: --text-*, --lh-*, --t-body, --t-field">
          <TypeScale />
        </Section>
        <Section id="colour" title="Colour" use="Colours are the palette's tokens, never a hex; accent means live, and a state is always said in words too." from="shared/style.css, shared/appearance.ts: legible()">
          <Colour scheme={shown} />
        </Section>
        <Section id="buttons" title="Buttons" use="Three heights; one primary to a place; danger beside a variant; the segmented control for a choice of one." from="shared/buttons.css">
          <Buttons />
        </Section>
        <Section id="fields" title="Fields" use="The quiet hairline: a field is outlined, a button is filled. A switch when it takes effect now, a box when a button commits it, a dot for one of a few that need words, segments for one of two to four, chips for days." from="shared/fields.css (or .field), shared/switch.css: .sw-switch, shared/ui/search-field.tsx, shared/ui/schedule.tsx, shared/buttons.css: .seg">
          <Fields />
        </Section>
        <Section id="headers" title="Headers" use="Two headers and nothing else: the bar for a pane, a block or a column; the page header for a place you go to." from="shared/ui/bar.tsx, shared/ui/page-header.tsx">
          <Headers />
        </Section>
        <Section id="rows" title="Rows and the selected state" use="One selected state on every row: the tint, a 3px accent bar and a 1px accent edge." from="shared/ui/list-page.tsx, shared/ui/card.tsx">
          <Rows />
        </Section>
        <Section id="chips" title="Chips" use="Five shapes, so a thing's shape says what it is: a state, a label, a number, a thing with an id, a tip." from="shared/ui/state-chip.tsx">
          <Chips />
        </Section>
        <Section id="cards" title="Cards" use="What a list or a page holds one of; a tone gives it a bar, an edge and a fill." from="shared/ui/card.tsx">
          <Cards />
        </Section>
        <Section id="icons" title="Icons" use="An icon is drawn from icons.tsx, in a size by name, and an icon alone is an IconButton with a label." from="shared/ui/icons.tsx">
          <Icons />
        </Section>
        <Section id="dialogs" title="Dialogs and side sheets" use="Dialogs are Modal, removing something asks in ConfirmRemove, and a panel beside the page is a SideSheet." from="shared/ui/modal.tsx, shared/ui/confirm.tsx, shared/ui/side-sheet.tsx">
          <Dialogs />
        </Section>
        <Section id="menus" title="Menus" use="One menu for every list of actions: it hangs from its button and is a bottom sheet on a phone." from="shared/ui/menu.tsx: openMenu">
          <Menus />
        </Section>
        <Section id="states" title="Empty and error states" use="A statement with a full stop, a why and at most one primary; no match is a dim line; an error starts with Can't." from="shared/ui/block-states.tsx">
          <States />
        </Section>
        <Section id="spacing" title="Spacing and shadows" use="Use the scale for space, the radius tokens for corners and the three shadows for depth." from="shared/style.css: --space-*, --*-radius, --shadow-*, --icon-*">
          <Spacing scheme={shown} />
        </Section>
      </main>
    </div>
  );
}
