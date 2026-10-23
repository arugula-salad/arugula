// What a project's Claude calls cost this month (M231), in its settings: the
// one place that shows Claude spend. Each line is an account's calls through
// the inference broker, made by them or by a registered client for them
// (studio's lobby), on one kind of key: the project's own, an account's own,
// or Arugula's (under a monthly cap per account). Agent turns on the
// project's hosted workspaces are counted beside them: their calls go
// through E2B's proxy, which doesn't see their usage.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { nameIn } from "../pages/issues";

interface Line {
  account: string | null;
  client: string | null;
  pays: "project" | "account" | "platform";
  calls: number;
  input_tokens: number;
  output_tokens: number;
  usd: number;
}

interface Spend {
  month: string;
  usd: number;
  platform_usd: number;
  lines: Line[];
  hosted: { turns: number; platform_turns: number; minutes: number };
}

const dollars = (usd: number) => `$${usd < 0.01 && usd > 0 ? usd.toFixed(4) : usd.toFixed(2)}`;
const count = (n: number) => n.toLocaleString();
const whose: Record<Line["pays"], string> = { project: "the project's key", account: "their own key", platform: "Arugula's key" };

export function ClaudeSpend({ p }: { p: PlatformProject }) {
  const [v, setV] = useState<Spend | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    platformFetch(`/api/projects/${encodeURIComponent(p.id)}/inference/spend`, { credentials: "same-origin" })
      .then(async (r) => {
        const j = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error((j as { error?: string }).error ?? `Request failed (${r.status})`);
        setV(j as Spend);
      })
      .catch((e: Error) => setErr(e.message));
  }, [p.id]);
  if (err) return <p class="control-error">{err}</p>;
  // Nothing to show where nothing went through Arugula this month.
  if (!v || (v.lines.length === 0 && v.hosted.turns === 0)) return null;
  const name = nameIn(p);
  const who = (a: string | null) => (a ? (name(a) ?? a) : "a deleted account");
  return (
    <div data-claude-spend={v.month}>
      <h2>Claude spend</h2>
      <p class="dim">
        What this project's Claude calls through Arugula cost in {v.month} (UTC), at Anthropic's list prices: {dollars(v.usd)}
        {v.platform_usd > 0 ? `, of which Arugula's key paid ${dollars(v.platform_usd)}` : ""}. Keys are never shown, here or anywhere.
      </p>
      {v.lines.length > 0 && (
        <ul class="platform-list" data-spend-lines>
          {v.lines.map((l) => (
            <li key={`${l.account}-${l.client}-${l.pays}`} data-spend-line={l.pays}>
              {who(l.account)}
              {l.client ? <span class="dim"> · through {l.client}</span> : null} · {dollars(l.usd)}
              <span class="dim">
                {" "}
                · {count(l.calls)} {l.calls === 1 ? "call" : "calls"}, {count(l.input_tokens)} in, {count(l.output_tokens)} out, on {whose[l.pays]}
              </span>
            </li>
          ))}
        </ul>
      )}
      {v.hosted.turns > 0 && (
        <p class="dim" data-spend-hosted>
          Hosted workspaces: {count(v.hosted.turns)} agent {v.hosted.turns === 1 ? "turn" : "turns"} ({count(v.hosted.minutes)} min)
          {v.hosted.platform_turns > 0 ? `, ${count(v.hosted.platform_turns)} on Arugula's key` : ""}. Their tokens aren't priced here.
        </p>
      )}
    </div>
  );
}
