// Doc blocks (#963): a document an agent builds beside its pane, an outline
// first and then one section at a time. One scrolling column: the title,
// then each section's heading and either its body (Markdown, as the forge
// blocks draw one: no HTML ever) or a placeholder saying what it will hold.
// A section filled while you watch gets a short highlight; nothing scrolls
// you. The daemon pushes every change (patches that `client.ts` merges into
// the whole state), so there is nothing here to poll.

import { render } from "preact";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import { Markdown } from "../ui/markdown";
import { registerBlock, type BlockView } from "./view";

interface Section {
  id: string;
  heading: string;
  intent: string;
  /** `null` while pending. */
  body: string | null;
  /** Bumped when the body is written, a refill too. */
  rev: number;
}

interface DocState {
  title: string;
  entries: Section[];
  rev: number;
  error?: string;
}

/** The Doc as Markdown, as the daemon's `capture --text` has it. */
function markdown(s: DocState): string {
  const parts = [`# ${s.title}\n`];
  for (const e of s.entries) {
    const body = e.body !== null ? `${e.body.trimEnd()}\n` : `_(coming${e.intent ? `: ${e.intent}` : ""})_\n`;
    parts.push(`\n## ${e.heading}\n\n${body}`);
  }
  return parts.join("");
}

function DocBlock({ id, s, fresh }: { id: PaneId; s: DocState | null; fresh: Map<string, number> }) {
  if (!s) return <div class="browser-card dim">Loading…</div>;
  if (s.error) {
    return (
      <div class="browser-card error" data-doc-block={id}>
        <p>Can't show this Doc</p>
        <p class="dim">{s.error}</p>
      </div>
    );
  }
  const done = s.entries.filter((e) => e.body !== null).length;
  return (
    <div class="doc" data-doc-block={id}>
      <div class="doc-page">
        <h1 class="doc-title">{s.title || "Untitled"}</h1>
        {s.entries.length === 0 && <p class="dim doc-wait">Waiting for the outline…</p>}
        {s.entries.length > 0 && (
          <p class="dim doc-progress" data-doc-progress>
            {done} of {s.entries.length} sections written
          </p>
        )}
        {s.entries.map((e) => (
          <section key={e.id} class="doc-section" data-section={e.id} data-pending={e.body === null ? "" : undefined}>
            <h2 class="doc-heading">{e.heading}</h2>
            {e.body !== null ? (
              // A refill is a new element, so its highlight plays again.
              <div key={e.rev} class={fresh.get(e.id) === e.rev ? "doc-body doc-fresh" : "doc-body"}>
                <Markdown text={e.body} refs={null} />
              </div>
            ) : (
              <div class="doc-pending">
                <span class="doc-coming">Coming…</span>
                {e.intent && <span class="doc-intent"> {e.intent}</span>}
              </div>
            )}
          </section>
        ))}
      </div>
    </div>
  );
}

registerBlock("doc", (client: Client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-doc";
  let state: DocState | null = null;
  /** The rev each section had when it was filled while this page watched. */
  const fresh = new Map<string, number>();
  let note = false;
  const draw = () => {
    // The daemon's features arrive just after its layout. Without the flag a
    // saved Doc is still on the machine, but it isn't shown (#347).
    if (client.features !== null && !client.flag("docs")) {
      host.className = "block block-unknown";
      render(null, host);
      host.textContent = "This block is in Labs";
      note = true;
      return;
    }
    if (note) host.textContent = "";
    note = false;
    host.className = "block block-doc";
    render(<DocBlock id={id} s={state} fresh={fresh} />, host);
  };
  const off = client.subscribe(draw);
  draw();
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (next) => {
      const s = next as DocState;
      // The first state we get is the baseline: only what changes after it
      // is a fill we watched.
      if (state) {
        const had = new Map(state.entries.map((e) => [e.id, e.rev]));
        for (const e of s.entries) if (e.rev > (had.get(e.id) ?? 0)) fresh.set(e.id, e.rev);
      }
      state = s;
      draw();
    },
    title: () => state?.title || "Doc",
    text: () => (state ? markdown(state) : ""),
    focus: () => host.querySelector<HTMLElement>(".doc-page")?.focus(),
    dispose: () => {
      off();
      render(null, host);
      host.remove();
    },
  };
});
