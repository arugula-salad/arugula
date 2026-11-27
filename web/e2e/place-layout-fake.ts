// A made-up place-layout server for the board's specs (Lift 5b, Arugula
// #1497): `/api/layouts/{place}` as the platform answers it (#1496's
// record and ownership rules, simply), and the lead's side, which
// changes the record and says so on the stream as a `layout` event.
import type { Page } from "@playwright/test";
import type { LayoutChange, LayoutNewPane, LayoutPane, LayoutTab, PlaceLayout } from "../src/shared/proto.gen";

const LEAD = { account: "acct_a", machine: "mch_dust" };
const YOU = { account: "acct_a", machine: null };

export interface FakeLayouts {
  /** The record now. */
  layout: PlaceLayout;
  /** Every change the page posted. */
  posted: LayoutChange[];
  /** The lead shows a pane (`beside` or `for_you`), as MCP `show` with a place target does; the page hears a `layout` event. */
  lead(pane: Partial<LayoutNewPane> & Pick<LayoutNewPane, "kind" | "ref" | "title">, intent?: "beside" | "for_you", inPlaceOf?: string): Promise<{ pane: string; tab: string }>;
  /** Say the record changed, without changing it (the stream again). */
  say(): Promise<void>;
}

export async function fakeLayouts(page: Page, place: string): Promise<FakeLayouts> {
  let n = 0;
  const id = (p: string) => `${p}_${++n}`;
  const f: FakeLayouts = {
    layout: { place, rev: 0, tabs: [{ id: "home", title: "", opened_by: YOU, opened_at: 0, for_you: false, panes: [] }], closed_defaults: [], default_sizes: {}, closed: [], changed: null },
    posted: [],
    async lead(np, intent = "beside", inPlaceOf) {
      const r = apply({ op: "show", pane: { machine: null, state: null, ...np }, intent, tab: null, in_place_of: inPlaceOf ?? null }, true);
      await f.say();
      return r as { pane: string; tab: string };
    },
    async say() {
      await page.evaluate(([place, rev]) => (window as unknown as { __chat: { heard(ev: unknown): void } }).__chat.heard({ type: "layout", place, rev }), [place, f.layout.rev] as const);
    },
  };
  const tab = (t: string) => f.layout.tabs.find((x) => x.id === t);
  const where = (pane: string): [LayoutTab, LayoutPane] | null => {
    for (const t of f.layout.tabs) for (const p of t.panes) if (p.id === pane) return [t, p];
    return null;
  };
  const changed = (agent: boolean) => (f.layout.changed = { by: agent ? LEAD : YOU, at: Date.now() });
  const closeOne = (t: LayoutTab, p: LayoutPane) => {
    t.panes = t.panes.filter((x) => x !== p);
    f.layout.closed.unshift(p);
  };
  function apply(c: LayoutChange, agent: boolean): { pane?: string; tab?: string; closed?: LayoutPane[]; refused?: string } {
    const l = f.layout;
    const by = agent ? LEAD : YOU;
    const out: { pane?: string; tab?: string; closed?: LayoutPane[] } = {};
    switch (c.op) {
      case "show": {
        const p: LayoutPane = { id: id("pan"), kind: c.pane.kind, ref: c.pane.ref, title: c.pane.title, machine: c.pane.machine, state: c.pane.state, opened_by: by, opened_at: Date.now() + n, touched: false, in_place_of: c.in_place_of, size: null, offline: false };
        let t = c.tab ? tab(c.tab)! : l.tabs[0]!;
        if (c.intent === "for_you") {
          t = { id: id("tab"), title: c.pane.title, opened_by: by, opened_at: Date.now(), for_you: true, panes: [] };
          l.tabs.push(t);
        }
        t.panes.push(p);
        // At most three agent panes a tab: a fourth closes the oldest the agent opened that you haven't touched.
        out.closed = [];
        while (agent && t.panes.filter((x) => x.opened_by.machine).length > 3) {
          const old = t.panes.find((x) => x.opened_by.machine && !x.touched);
          if (!old) break;
          closeOne(t, old);
          out.closed.push(old);
        }
        if (t.id === "home") changed(agent);
        Object.assign(out, { pane: p.id, tab: t.id });
        break;
      }
      case "move": {
        const w = where(c.pane)!;
        w[0].panes = w[0].panes.filter((x) => x !== w[1]);
        if (!agent) w[1].touched = true;
        const t = c.tab ? tab(c.tab)! : { id: id("tab"), title: w[1].title, opened_by: by, opened_at: Date.now(), for_you: false, panes: [] };
        if (!c.tab) l.tabs.push(t);
        t.panes.push(w[1]);
        if (w[0].id === "home") changed(agent);
        Object.assign(out, { pane: w[1].id, tab: t.id });
        break;
      }
      case "resize":
      case "maximise": {
        const w = where(c.pane);
        if (w) {
          w[1].touched = true;
          if (c.op === "resize") w[1].size = c.size;
        } else if (c.op === "resize") l.default_sizes[c.pane] = c.size;
        changed(false);
        break;
      }
      case "close": {
        const w = where(c.pane);
        if (w) closeOne(w[0], w[1]);
        else l.closed_defaults.push(c.pane);
        if (!w || w[0].id === "home") changed(agent);
        break;
      }
      case "close_tab": {
        const t = tab(c.tab)!;
        l.tabs = l.tabs.filter((x) => x !== t);
        l.closed.unshift(...t.panes);
        break;
      }
      case "reopen": {
        const p = l.closed.find((x) => x.id === c.pane)!;
        l.closed = l.closed.filter((x) => x !== p);
        l.tabs[0]!.panes.push(p);
        changed(agent);
        break;
      }
      case "reset": {
        const home = l.tabs[0]!;
        l.closed.unshift(...home.panes);
        home.panes = [];
        l.closed_defaults = [];
        l.default_sizes = {};
        l.changed = null;
        break;
      }
    }
    l.rev++;
    return out;
  }
  await page.route(`**/api/layouts/${encodeURIComponent(place)}`, async (r) => {
    if (r.request().method() === "GET") return r.fulfill({ json: f.layout });
    const c = r.request().postDataJSON() as LayoutChange;
    f.posted.push(c);
    const out = apply(c, false);
    return r.fulfill({ json: { layout: f.layout, pane: out.pane ?? null, tab: out.tab ?? null, closed: out.closed ?? [] } });
  });
  return f;
}
