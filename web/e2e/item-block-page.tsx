// The fixture page for the item block (#407): every state it draws, side by
// side, from plain data, so the specs need no daemon, forge or control.
// item-block-steps.ts builds this with Vite and serves it from the test.

import { render } from "preact";
import "../src/shared/style.css";
import "../src/shared/buttons.css";
import { ItemBlock, type ItemFact } from "../src/shared/ui/item-block";

const LONG =
  "One full CI run for six reviewed PRs, so CI runs once, not six times. Squash-merge keeps main linear, and the rail lands every change behind the same gate. " +
  "The second paragraph explains the migration: callers move one at a time, each keeping its old look until its own ticket lands, so nothing here is visible to a person yet. " +
  "The third says what was left out on purpose and why the fourth ticket picks it up.";

const prFacts = (extra: ItemFact[]): ItemFact[] => [{ kind: "branch", from: "land/rail-and-ux", to: "main" }, ...extra, { kind: "comments", count: 3 }];

const Btn = ({ children, kind = "secondary" }: { children: string; kind?: string }) => (
  <button type="button" class={`${kind} small`}>
    {children}
  </button>
);

const refresh = { onRefresh: () => {}, refreshTitle: "Read 2m ago", href: "https://example.test/1" };

function Page() {
  return (
    <main style="display:grid;gap:24px;max-width:760px;margin:24px auto">
      <div data-fixture="pr-needs-you">
        <ItemBlock
          kind="pr"
          number={1871}
          title="Land: one rail everywhere and four UX fixes"
          state="open"
          source={{ kind: "github", live: "following", title: "GitHub · following, as alecraso (gh)" }}
          {...refresh}
          next={{ tone: "danger", text: "e2e (10) failed", actions: <><Btn kind="primary">Rerun</Btn><Btn>Open run ↗</Btn></> }}
          facts={prFacts([
            { kind: "checks", ok: 29, pending: 1, failed: 1 },
            { kind: "reviewers", people: [{ name: "jake", state: "approved" }, { name: "alec", state: "asked" }] },
            { kind: "mergeable", state: "clean" },
          ])}
          body={LONG}
          tabs={[
            { id: "checks", label: "Checks", count: 31, tone: "danger", content: <p>e2e (10) failed</p> },
            { id: "reviews", label: "Reviews", count: 2, content: <p>jake approved</p> },
            { id: "activity", label: "Activity", count: 7, content: <p>Seven events</p> },
          ]}
          footer={<><Btn>Comment</Btn><Btn>Diff</Btn><Btn>Checkout</Btn></>}
        />
      </div>
      <div data-fixture="pr-ready">
        <ItemBlock
          kind="pr"
          number={1880}
          title="Boards: Copy link"
          state="open"
          source={{ kind: "github", live: "paused", title: "GitHub · paused" }}
          {...refresh}
          next={{ tone: "ok", text: "Ready to merge", actions: <Btn kind="primary">Merge</Btn> }}
          facts={prFacts([
            { kind: "checks", ok: 31 },
            { kind: "reviewers", people: [{ name: "jake", state: "approved", stale: true }] },
            { kind: "mergeable", state: "clean" },
          ])}
          body="Adds the button."
          footer={<><Btn>Comment</Btn><Btn kind="primary">Merge ▾</Btn></>}
        />
      </div>
      <div data-fixture="pr-merged">
        <ItemBlock
          kind="pr"
          number={1862}
          title="Attach: keep the picker's input in the page"
          state="merged"
          source={{ kind: "github", live: "following" }}
          {...refresh}
          facts={[
            { kind: "branch", from: "fix/attach", to: "main" },
            { kind: "checks", ok: 31 },
            { kind: "reviewers", people: [{ name: "jake", state: "approved" }] },
            { kind: "text", text: "merged 1cc9e39f · 2h", title: "Merged as 1cc9e39f two hours ago" },
            { kind: "comments", count: 1 },
          ]}
          body="The picker kept its input out of the page."
          tabs={[
            { id: "reviews", label: "Reviews", count: 1, content: <p>jake approved</p> },
            { id: "activity", label: "Activity", count: 4, content: <p>Four events</p> },
          ]}
          footer={<Btn>Diff</Btn>}
        />
      </div>
      <div data-fixture="pr-closed">
        <ItemBlock
          kind="pr"
          number={1850}
          title="Try a second rail"
          state="closed"
          source={{ kind: "forgejo", live: null }}
          facts={[{ kind: "branch", from: "try/rail", to: "main" }, { kind: "mergeable", state: "conflicts" }]}
          body="Dropped in favour of #1871."
        />
      </div>
      <div data-fixture="pr-draft">
        <ItemBlock
          kind="pr"
          number={1890}
          title="Agents hear their mentions"
          state="draft"
          source={{ kind: "gitlab", live: "following" }}
          facts={prFacts([{ kind: "checks", pending: 2 }, { kind: "mergeable", state: "checking" }])}
          body="Not ready yet."
          footer={<Btn>Comment</Btn>}
        />
      </div>
      <div data-fixture="issue-question">
        <ItemBlock
          kind="issue"
          number={407}
          title="Item blocks 1/4: a shared ItemBlock component"
          state="open"
          source={{ kind: "arugula", live: "following" }}
          {...refresh}
          next={{ tone: "warn", text: "The agent asks: which pill colour for Draft?", actions: <Btn kind="primary">Answer</Btn> }}
          facts={[{ kind: "labels", labels: ["web", "design"] }, { kind: "assignees", people: ["alec"] }, { kind: "comments", count: 2 }]}
          body="Draws the six parts from data."
          tabs={[
            { id: "activity", label: "Activity", count: 5, content: <p>Five events</p> },
            { id: "agents", label: "Agents", count: 1, tone: "warn", content: <p>One agent is waiting</p> },
          ]}
          footer={<Btn>Comment</Btn>}
        />
      </div>
      <div data-fixture="issue-empty">
        <ItemBlock
          kind="issue"
          number={408}
          title="A bare issue"
          state="open"
          source={{ kind: "github", live: "following" }}
          facts={[{ kind: "comments", count: 0 }]}
          body=""
          footer={<Btn>Comment</Btn>}
        />
      </div>
    </main>
  );
}

render(<Page />, document.getElementById("root")!);
