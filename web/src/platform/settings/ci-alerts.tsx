// CI alerts (native #774), a section of a project's settings: chant's tick
// posts its CI events (`ci.red`, `ci.culprit`, `ci.stale`, `ci.green`) to
// the URL here, signed with the project's key, and they open, comment on and
// close a "main is red" native issue, written by chant CI. Turning it on
// makes the key, which is shown once, here, until the page is left; Rotate
// makes a new one and the old stops at once; Turn off removes it. Who a new
// red issue mentions (and assigns) is picked from the project's members. A
// team's viewers see it all but the buttons.

import { useEffect, useState } from "preact/hooks";
import type { CiAlerts, CiAlertsKey } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import { ago } from "../../shared/time";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { Section } from "../../shared/ui/settings-section";
import type { PlatformProject } from "../session";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { membersOf, mayWrite } from "../pages/issues";

/** A call to the platform: its JSON, or its `{error}` thrown. */
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
  return v as T;
}

export function CiAlertsSetting({ p }: { p: PlatformProject }) {
  const [a, setA] = useState<CiAlerts | null>(null);
  // The key, from the answer that made it: shown until the page is left.
  const [key, setKey] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [offing, setOffing] = useState(false);
  const base = `/api/projects/${encodeURIComponent(p.id)}/ci-alerts`;
  useEffect(() => {
    call<CiAlerts>("GET", base).then(setA, (e: Error) => setErr(e.message));
  }, [p.id]);
  if (!a) return err ? <p class="control-error">{err}</p> : null;
  const changes = mayWrite(p);
  const act = async (f: () => Promise<CiAlerts>) => {
    setBusy(true);
    setErr("");
    try {
      setA(await f());
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const makeKey = () =>
    act(async () => {
      const v = await call<CiAlertsKey>("POST", `${base}/key`);
      setKey(v.key);
      return v.alerts;
    });
  const members = membersOf(p);
  const nameOf = (account: string) => members.find((m) => m.account === account)?.name ?? account;
  // Shown at once, then as the platform answers (a refusal puts it back).
  const change = (to: Partial<Pick<CiAlerts, "people" | "assign">>) => {
    const was = a;
    setA({ ...a, ...to });
    void act(() => call<CiAlerts>("PATCH", base, to).catch((e: Error) => {
      setA(was);
      throw e;
    }));
  };
  const setPeople = (people: string[]) => change({ people });
  return (
    <Section
      id="ci-alerts"
      title="CI alerts"
      lede="A red main opens an issue here, chant's CI events comment on it, and a green commit closes it."
      hooks={{ "data-ci-alerts": a.on ? "on" : "off" }}
      more={
        <>
          <p>
            chant's tick sends its CI events to the URL below, signed with this project's key. A red branch opens one issue, written by chant CI rather than by a
            person: a culprit, the branch staying red, and red again comment on it, and the next green commit comments and closes it.
          </p>
          <p>
            In <code>chant.workspace.json</code>, <code>ci.green.notify</code> names two environment variables, <code>webhook</code> for the URL and{" "}
            <code>key</code> for the key. Set each as a repository secret of that name; <code>chant ci workflow</code> gives them to the tick.
          </p>
        </>
      }
    >
      {!a.on ? (
        changes ? (
          <button class="primary" data-ci-alerts-on disabled={busy} onClick={() => void makeKey()}>
            Turn on
          </button>
        ) : (
          <p class="dim">Off. A team's owners and editors turn it on.</p>
        )
      ) : (
        <div class="plane-hook" data-ci-alerts-hook>
          <span class="dim">URL</span>
          <CopyText text={a.url} data-ci-alerts-url />
          {key ? (
            <>
              <span class="dim">Key, shown this once: set it as the secret chant reads now.</span>
              <CopyText text={key} data-ci-alerts-key />
            </>
          ) : (
            <span class="dim" data-ci-alerts-key-made>
              Key made {a.key_made_at ? ago(a.key_made_at, Date.now(), "long") : ""}
              {a.key_made_by ? ` by ${nameOf(a.key_made_by)}` : ""}. It isn't shown again: rotate it to get a new one.
            </span>
          )}
          <span class="dim" data-ci-alerts-last>
            {a.last_event_at ? `Last event ${ago(a.last_event_at, Date.now(), "long")}.` : "No event yet."}
          </span>
          {changes && (
            <span class="plane-hook-form">
              <button class="secondary small" data-ci-alerts-rotate disabled={busy} onClick={() => void makeKey()}>
                Rotate key
              </button>
              <button class="secondary danger small" data-ci-alerts-off disabled={busy} onClick={() => setOffing(true)}>
                Turn off
              </button>
            </span>
          )}
          <fieldset class="ci-alerts-people" data-ci-alerts-people>
            <legend>Mention on a new red issue</legend>
            {members.map((m) => (
              <label class="settings-switch" key={m.account}>
                <input
                  type="checkbox"
                  data-ci-alerts-person={m.account}
                  checked={a.people.includes(m.account)}
                  disabled={busy || !changes}
                  onChange={(e) => {
                    const on = e.currentTarget.checked;
                    setPeople(on ? [...a.people, m.account] : a.people.filter((x) => x !== m.account));
                  }}
                />
                <span>{m.name}</span>
              </label>
            ))}
            <label class="settings-switch">
              <input
                type="checkbox"
                role="switch"
                class="sw-switch"
                data-ci-alerts-assign
                checked={a.assign}
                disabled={busy || !changes}
                onChange={(e) => {
                  change({ assign: e.currentTarget.checked });
                }}
              />
              <span>Assign them too</span>
            </label>
            <p class="dim">They're mentioned again when a culprit is named, when the branch stays red, and when it's green. Their push settings apply.</p>
          </fieldset>
          {a.open.length > 0 && (
            <ul class="platform-list settings-rows" data-ci-alerts-open>
              {a.open.map((o) => {
                const href = pathOf({ page: "issue", project: p.id, issue: o.issue }, names);
                return (
                  <li key={o.branch} data-ci-alerts-open-branch={o.branch}>
                    <a href={href} onClick={(e) => follow(e, href)}>
                      #{o.number}
                    </a>
                    <span>
                      <b>{o.branch}</b> is red, since {ago(o.opened_at, Date.now(), "long")}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
      {err && <p class="control-error" data-ci-alerts-error>{err}</p>}
      {offing && (
        <ConfirmRemove
          title="Turn CI alerts off?"
          label="Turn off"
          cancel={() => setOffing(false)}
          go={() => {
            setOffing(false);
            setKey(null);
            void act(() => call<CiAlerts>("PATCH", base, { on: false }));
          }}
        >
          <p>The key stops working at once and is removed. The issues stay; turning it on again makes a new key.</p>
        </ConfirmRemove>
      )}
    </Section>
  );
}
