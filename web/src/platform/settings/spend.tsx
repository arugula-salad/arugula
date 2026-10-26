// What a project's Claude calls cost this month (M231), in its settings: the
// one place that shows Claude spend. Each line is an account's calls through
// the inference broker, made by them or by a registered client for them
// (studio's lobby), on one kind of key: the project's own, an account's own,
// or Arugula's (under a monthly cap per account). Agent turns on the
// project's hosted workspaces are counted beside them: their calls go
// through E2B's proxy, which doesn't see their usage. A section
// (./section): the month's totals as the swarm's stats, then the lines as
// a table.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { nameIn } from "../pages/issues";
import { Section } from "./section";

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
    <Section
      id="spend"
      title="Claude spend"
      hooks={{ "data-claude-spend": v.month }}
      lede={`What this project's Claude calls through Arugula cost in ${v.month} (UTC).`}
      more={
        <p>
          At Anthropic's list prices, by who made the calls (or a product, for them) and whose key paid: the project's, their own, or Arugula's,
          which has a monthly cap per account. Agent turns on the project's cloud workspaces are counted beside them: their calls go through
          E2B's proxy, which doesn't see their usage, so their tokens aren't priced here. Keys are never shown, here or anywhere.
        </p>
      }
    >
      <div class="sw-stats">
        <span>
          <b>{dollars(v.usd)}</b>this month
        </span>
        {v.platform_usd > 0 ? (
          <span>
            <b>{dollars(v.platform_usd)}</b>on Arugula's key
          </span>
        ) : null}
        {v.hosted.turns > 0 ? (
          <span>
            <b>{count(v.hosted.turns)}</b>hosted agent {v.hosted.turns === 1 ? "turn" : "turns"}
          </span>
        ) : null}
      </div>
      {v.lines.length > 0 && (
        <table class="settings-table spend-table" data-spend-lines>
          <thead>
            <tr>
              <th scope="col">Who</th>
              <th scope="col">Key</th>
              <th scope="col">Calls</th>
              <th scope="col">In</th>
              <th scope="col">Out</th>
              <th scope="col">Cost</th>
            </tr>
          </thead>
          <tbody>
            {v.lines.map((l) => (
              <tr key={`${l.account}-${l.client}-${l.pays}`} data-spend-line={l.pays}>
                <th scope="row">
                  {who(l.account)}
                  {l.client ? <span class="dim"> · through {l.client}</span> : null}
                </th>
                <td data-col="Key">{whose[l.pays]}</td>
                <td class="num" data-col="Calls">
                  {count(l.calls)}
                </td>
                <td class="num" data-col="In">
                  {count(l.input_tokens)}
                </td>
                <td class="num" data-col="Out">
                  {count(l.output_tokens)}
                </td>
                <td class="num spend-usd" data-col="Cost">
                  {dollars(l.usd)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {v.hosted.turns > 0 && (
        <p class="dim settings-note" data-spend-hosted>
          Cloud workspaces: {count(v.hosted.turns)} agent {v.hosted.turns === 1 ? "turn" : "turns"} ({count(v.hosted.minutes)} min)
          {v.hosted.platform_turns > 0 ? `, ${count(v.hosted.platform_turns)} on Arugula's key` : ""}. Their tokens aren't priced here.
        </p>
      )}
    </Section>
  );
}
