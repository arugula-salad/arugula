// A project's standing grant for runs (M211), in its settings: whether the
// platform may start and instruct runs on the project's hosted workspaces
// with every device asleep, confirmed by whose passkey (DK6; a device
// signed the ones from before) and when, and what was revoked. Allowing
// and revoking take a passkey of the account's, made for control's site,
// so they're on control's page (here or on the phone), and this links
// there. (Moving them here, by WebAuthn's related origins as DK5 does for
// deploys, is a follow-up.) The grant
// in place is a card: its bar ok, or warn when workspaces enrolled since
// refuse it or it expires soon. Shown on every project, with nothing in
// place too (UI audit #166: a project's settings show every section).

import { Card } from "../../shared/ui/card";
import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { Time } from "../../shared/ui/time";
import { controlOrigin } from "../session";
import { Section } from "../../shared/ui/settings-section";
import { Chip } from "../../shared/ui/state-chip";

interface Grant {
  id: string;
  made: number;
  expires: number | null;
  by: string;
  /** v2 (DK6): the owner whose passkey confirmed it. */
  account?: string;
}

interface View {
  role: "owner" | "editor" | "viewer";
  grant: { grant: Grant; created_at: number } | null;
  past: { grant: Grant; revoke: { running: "finish" | "stop"; at: number } | null; revoked_at: number | null; superseded_at: number | null }[];
  stale: boolean;
  renew: boolean;
  /** The owner who confirmed it isn't an owner now: machines refuse it.
   * A platform from before leaves it out. */
  signer_gone?: boolean;
  workspaces: number;
  offer: boolean;
}

export function RunGrant({ project }: { project: string }) {
  const [v, setV] = useState<View | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    platformFetch(`/api/projects/${encodeURIComponent(project)}/runs/grant`, { credentials: "same-origin" })
      .then(async (r) => {
        const j = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error((j as { error?: string }).error ?? `Request failed (${r.status})`);
        setV(j as View);
      })
      .catch((e: Error) => setErr(e.message));
  }, [project]);
  if (err) return <p class="control-error">{err}</p>;
  if (!v) return null;
  const g = v.grant?.grant;
  const sign = (
    <a href={controlOrigin()} data-run-grant-sign>
      your machines page
    </a>
  );
  return (
    <Section
      id="runs"
      title="Runs"
      hooks={{ "data-run-grant": g ? "yes" : v.offer ? "offered" : "no" }}
      lede={
        g
          ? "Arugula may start runs on this project's cloud workspaces while every device sleeps."
          : "No runs start here: nobody has allowed them, or the last grant expired or was revoked."
      }
      more={
        <p>
          A standing grant lets Arugula start runs and instruct them on every cloud workspace of this project, running whatever command
          the project's runs start, with every device asleep, until it expires or is revoked. A project owner confirms it with a passkey, and each
          workspace checks that owner's passkey before it starts anything. Deploys still ask an owner every time.
        </p>
      }
    >
      {g ? (
        <ul class="platform-list card-list settings-cards">
          <Card as="li" tone={v.stale || v.renew || v.signer_gone ? "warn" : "ok"}>
            <div class="card-head">
              <b class="card-title big">Standing grant</b>
              <Chip kind="tag">
                {g.expires ? (
                  <>
                    expires <Time at={g.expires} />
                  </>
                ) : (
                  "until it's revoked"
                )}
              </Chip>
            </div>
            <span class="dim">
              {g.account ? "A project owner confirmed it with a passkey" : "A project owner's device signed it"} <Time at={g.made} on />.
            </span>
            {v.stale ? (
              <span class="settings-warn">It's for an earlier delegation, so workspaces enrolled since refuse it: sign it again.</span>
            ) : v.signer_gone ? (
              <span class="settings-warn" data-run-grant-signer-gone>
                The owner who confirmed it isn't an owner of the project now, so workspaces refuse it: sign it again.
              </span>
            ) : v.renew ? (
              <span class="settings-warn">It expires soon: renew it to keep runs starting.</span>
            ) : null}
          </Card>
        </ul>
      ) : null}
      {v.role === "owner" ? (
        <p class="dim settings-note">
          {g ? (v.stale || v.renew || v.signer_gone ? "Renew it, or revoke it," : "Revoke it") : v.offer ? "This project has cloud workspaces. Allow runs on them" : "Allow runs"} on{" "}
          {sign}, under Devices and machines, on a computer or your phone: you confirm it there with a passkey.
        </p>
      ) : (
        <p class="dim settings-note">A project owner allows runs, or revokes them, with a passkey.</p>
      )}
      {v.past.length ? (
        <ul class="platform-list dim settings-past" data-run-grants-past>
          {v.past.map((p) => (
            <li key={p.grant.id}>
              Allowed <Time at={p.grant.made} on />,{" "}
              {p.revoked_at ? (
                <>
                  revoked <Time at={p.revoked_at} on />
                  {p.revoke?.running === "stop" ? ", and the runs going stopped" : ""}
                </>
              ) : p.superseded_at ? (
                <>
                  replaced <Time at={p.superseded_at} on />
                </>
              ) : (
                "expired"
              )}
            </li>
          ))}
        </ul>
      ) : null}
    </Section>
  );
}
