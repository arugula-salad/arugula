// What an app has used (#318 P9's /usage): this month's active users
// against its cap, call minutes, and the last 31 days as two small bar
// charts (one measure each, no second axis), with a table for reading the
// numbers. Setting the cap. A relay that doesn't meter yet answers 404:
// that's a calm "nothing yet", not an error.

import { useEffect, useState } from "preact/hooks";
import * as api from "./api";
import type { App, Usage } from "./api";
import { Field, Section, value } from "./ui";

const fmt = (n: number) => n.toLocaleString();
const dayLabel = (d: string) => new Date(`${d}T12:00:00Z`).toLocaleDateString([], { month: "short", day: "numeric", timeZone: "UTC" });
const monthLabel = (m: string) => new Date(`${m}-15T12:00:00Z`).toLocaleDateString([], { month: "long", year: "numeric", timeZone: "UTC" });

/** One measure over the days: thin bars from a baseline, the top value
 * marked, and the day under the pointer read out above. */
function DayBars({ days, pick, unit, colour, title }: { days: Usage["days"]; pick: (d: Usage["days"][number]) => number; unit: string; colour: string; title: string }) {
  const [at, setAt] = useState<number | null>(null);
  const W = 620;
  const H = 96;
  const n = Math.max(days.length, 1);
  const slot = W / n;
  const bar = Math.max(slot - 2, 1);
  const most = Math.max(0, ...days.map(pick));
  const top = Math.max(most, 1);
  const shown = at === null ? null : days[at];
  return (
    <figure class="chart">
      <figcaption>
        <span>{title}</span>
        <span class="read" aria-live="polite">
          {shown ? `${dayLabel(shown.day)} · ${fmt(pick(shown))} ${unit}` : `most in a day: ${fmt(most)}`}
        </span>
      </figcaption>
      <svg viewBox={`0 0 ${W} ${H + 1}`} role="img" aria-label={`${title}, the last ${days.length} days`} onMouseLeave={() => setAt(null)}>
        <line x1="0" x2={W} y1={H + 0.5} y2={H + 0.5} class="base" />
        {days.map((d, i) => {
          const v = pick(d);
          const h = v ? Math.max((v / top) * (H - 4), 2) : 0;
          const x = i * slot + 1;
          const r = Math.min(2, bar / 2, h);
          return (
            <g key={d.day} onMouseEnter={() => setAt(i)} onTouchStart={() => setAt(i)}>
              <rect x={i * slot} y="0" width={slot} height={H} fill="transparent" />
              {h > 0 && <path d={`M${x},${H} v${-(h - r)} q0,${-r} ${r},${-r} h${bar - 2 * r} q${r},0 ${r},${r} v${h - r} z`} fill={colour} opacity={at === null || at === i ? 1 : 0.45} />}
              <title>{`${dayLabel(d.day)}: ${fmt(v)} ${unit}`}</title>
            </g>
          );
        })}
      </svg>
      <div class="axis" aria-hidden="true">
        <span>{days[0] ? dayLabel(days[0].day) : ""}</span>
        <span>{days.at(-1) ? dayLabel(days.at(-1)!.day) : ""}</span>
      </div>
    </figure>
  );
}

function Meter({ used, cap, own }: { used: number; cap: number | null; own: number | null }) {
  if (cap === null)
    return (
      <div class="tile">
        <span class="label">Active users this month</span>
        <b class="big">{fmt(used)}</b>
        <span class="hint">No cap set.</span>
      </div>
    );
  const planCap = own === null || cap < own;
  const share = cap > 0 ? used / cap : 1;
  const state = share >= 1 ? "at" : share >= 0.8 ? "near" : "ok";
  return (
    <div class={`tile meter ${state}`}>
      <span class="label">Active users this month</span>
      <b class="big">
        {fmt(used)} <small>of {fmt(cap)}</small>
      </b>
      <span class="track" role="meter" aria-valuemin={0} aria-valuemax={cap} aria-valuenow={used} aria-label="Active users against your cap">
        <span class="fill" style={{ width: `${Math.min(share, 1) * 100}%` }} />
      </span>
      {state !== "ok" && <span class="state">{state === "at" ? (planCap ? "⚠ At your plan's cap" : "⚠ At your cap") : planCap ? "Near your plan's cap" : "Near your cap"}</span>}
      {planCap && <span class="hint">Your plan's cap. Upgrade under Plan and billing for more.</span>}
    </div>
  );
}

function Cap({ app, cap, onSaved }: { app: App; cap: number | null; onSaved: (cap: number | null) => void }) {
  const [draft, setDraft] = useState(cap === null ? "" : String(cap));
  const [err, setErr] = useState("");
  const [saved, setSaved] = useState(false);
  const save = async (to: number | null) => {
    try {
      await api.setCap(app.id, to);
      onSaved(to);
      setDraft(to === null ? "" : String(to));
      setErr("");
      setSaved(true);
    } catch (e) {
      setErr((e as Error).message);
      setSaved(false);
    }
  };
  return (
    <form
      class="fields cap"
      noValidate
      onSubmit={(e) => {
        e.preventDefault();
        const n = Number(draft.trim());
        if (!draft.trim() || !Number.isInteger(n) || n < 1) return setErr("A cap is a whole number of people, 1 or more.");
        void save(n);
      }}
    >
      <Field label="Cap on active users" error={err || undefined} hint="The most people you want in a month. Past it, people who haven't been in this month can't join until the next one.">
        <input type="number" min={1} step={1} inputMode="numeric" placeholder="no cap" value={draft} onInput={(e) => (setDraft(value(e)), setSaved(false))} />
      </Field>
      <div class="row">
        <button class="btn" type="submit">
          Set the cap
        </button>
        {cap !== null && (
          <button class="btn small" type="button" onClick={() => void save(null)}>
            No cap
          </button>
        )}
        {saved && (
          <span class="ok" role="status">
            Saved.
          </span>
        )}
      </div>
    </form>
  );
}

export function UsageSection({ app }: { app: App }) {
  // undefined: loading; null: this relay doesn't meter yet.
  const [usage, setUsage] = useState<Usage | null | undefined>(undefined);
  const [err, setErr] = useState("");
  useEffect(() => {
    api.usage(app.id).then(setUsage, (e: Error) => setErr(e.message));
  }, [app.id]);
  return (
    <Section id="usage" title="Usage" kind="--k-build" sub="You pay for monthly active users: each person who's in any of your rooms in a month counts once, however many rooms or tabs.">
      {err ? (
        <p class="error">{err}</p>
      ) : usage === undefined ? (
        <p class="empty" aria-busy="true">
          Loading…
        </p>
      ) : usage === null ? (
        <p class="empty calm">Usage will appear here once people start joining your rooms.</p>
      ) : (
        <>
          <div class="tiles">
            <Meter used={usage.this_month.active_users} cap={usage.cap.limit === undefined ? usage.cap.active_users : usage.cap.limit} own={usage.cap.active_users} />
            <div class="tile">
              <span class="label">Call minutes this month</span>
              <b class="big">{fmt(usage.this_month.call_minutes)}</b>
            </div>
          </div>
          <DayBars days={usage.days} pick={(d) => d.active_users} unit="active users" colour="var(--k-server)" title="Active users a day" />
          <DayBars days={usage.days} pick={(d) => d.call_minutes} unit="call minutes" colour="var(--k-agent)" title="Call minutes a day" />
          <details class="more">
            <summary>As a table</summary>
            <table class="numbers">
              <thead>
                <tr>
                  <th>Month</th>
                  <th>Active users</th>
                  <th>Call minutes</th>
                </tr>
              </thead>
              <tbody>
                {usage.months.map((m) => (
                  <tr key={m.month}>
                    <td>{monthLabel(m.month)}</td>
                    <td>{fmt(m.active_users)}</td>
                    <td>{fmt(m.call_minutes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <table class="numbers">
              <thead>
                <tr>
                  <th>Day</th>
                  <th>Active users</th>
                  <th>Call minutes</th>
                </tr>
              </thead>
              <tbody>
                {[...usage.days].reverse().map((d) => (
                  <tr key={d.day}>
                    <td>{dayLabel(d.day)}</td>
                    <td>{fmt(d.active_users)}</td>
                    <td>{fmt(d.call_minutes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
          <Cap app={app} cap={usage.cap.active_users} onSaved={(c) => api.usage(app.id).then((u) => setUsage(u ?? { ...usage, cap: { active_users: c } }))} />
        </>
      )}
    </Section>
  );
}
