// A project's General settings (Arugula #895): its name, archiving it, and
// moving it to another owner, through `PATCH /api/projects/{project}`.
// Anyone who may write it renames it; only its owner archives or moves it,
// and a move needs the reader to own the new owner too. A move asks first:
// its people change, and the grants over it go. Its owner also moves some of
// its work elsewhere: "Move to project…" (Arugula #971, ../move).

import { useEffect, useState } from "preact/hooks";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { useSubscribe } from "../../shared/ui/hooks";
import { Section } from "../../shared/ui/settings-section";
import { names } from "../links";
import { makers, ownerKey, owners, PERSONAL } from "../owners";
import { follow, pathOf } from "../router";
import { session, type PlatformProject } from "../session";

export function General({ p }: { p: PlatformProject }) {
  useSubscribe((fn) => session.subscribe(fn));
  const all = owners(session.projects);
  const mine = all.find((o) => o.key === ownerKey(p));
  const role = mine?.role ?? "";
  const writes = role === "owner" || role === "editor";
  const owns = role === "owner";
  const elsewhere = makers(all).filter((o) => o.key !== mine?.key);
  const [name, setName] = useState(p.name);
  const [to, setTo] = useState(elsewhere[0]?.key ?? "");
  const [moving, setMoving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => setName(p.name), [p.name]);
  const target = elsewhere.find((o) => o.key === to) ?? elsewhere[0];
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr("");
    try {
      await f();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Section id="general" title="General" lede="Its name, whose it is, and whether it's archived." hooks={{ "data-project-general": p.id }}>
      <form
        class="settings-add"
        aria-label="Rename"
        onSubmit={(e) => {
          e.preventDefault();
          const n = name.trim();
          if (n && n !== p.name) void act(() => session.patch(p.id, { name: n }));
        }}
      >
        <input class="field" aria-label="Name" value={name} maxLength={100} disabled={!writes} data-project-name onInput={(e) => setName(e.currentTarget.value)} />
        {writes && (
          <button class="primary" type="submit" disabled={busy || !name.trim() || name.trim() === p.name} data-project-rename>
            Rename
          </button>
        )}
      </form>
      <p class="dim" data-project-owner>
        Owner: {mine?.name ?? "Personal"}
        {p.archived ? " · archived" : ""}
      </p>
      {owns ? (
        <>
          <p>
            <button class="secondary" disabled={busy} data-project-archive={p.archived ? "unarchive" : "archive"} onClick={() => void act(() => session.patch(p.id, { archived: !p.archived }))}>
              {p.archived ? "Unarchive" : "Archive"}
            </button>{" "}
            <span class="dim">{p.archived ? "It's back in the switcher's list." : "It leaves the switcher's list. Its issues, Docs and boards stay."}</span>
          </p>
          {target ? (
            <form
              class="settings-add"
              aria-label="Move"
              onSubmit={(e) => {
                e.preventDefault();
                setMoving(true);
              }}
            >
              <select class="field" aria-label="Move to" value={target.key} data-project-move-to onChange={(e) => setTo(e.currentTarget.value)}>
                {elsewhere.map((o) => (
                  <option key={o.key} value={o.key}>
                    {o.name}
                  </option>
                ))}
              </select>
              <button class="secondary" type="submit" disabled={busy} data-project-move>
                Move…
              </button>
            </form>
          ) : (
            <p class="dim">You own no other team to move it to.</p>
          )}
          <p>
            <MoveSome p={p} />{" "}
            <span class="dim">Some of its repos, boards, issues and Docs, into a new project or another one.</span>
          </p>
        </>
      ) : (
        <p class="dim">Its owners archive it and move it.</p>
      )}
      {err && (
        <p class="control-error" data-project-general-error>
          {err}
        </p>
      )}
      {moving && target && (
        <ConfirmRemove
          title={`Move ${p.name} to ${target.name}?`}
          label="Move"
          cancel={() => setMoving(false)}
          go={() => {
            setMoving(false);
            void act(() => session.patch(p.id, { owner: target.key === PERSONAL ? { kind: "personal" } : { kind: "team", id: target.key } }));
          }}
        >
          <p>
            Its issues, Docs, boards, linked repos and workspaces go with it. Who can use it becomes{" "}
            {target.kind === "personal" ? "you alone" : `${target.name}'s people`}.
          </p>
          <p>Every grant over it, machines' and agent tokens', is revoked. Give them again after the move.</p>
        </ConfirmRemove>
      )}
    </Section>
  );
}

/** "Move to project…": its own page. */
function MoveSome({ p }: { p: PlatformProject }) {
  const to = pathOf({ page: "move", project: p.id }, names);
  return (
    <a class="secondary" href={to} data-project-move-some onClick={(e) => follow(e, to)}>
      Move to project…
    </a>
  );
}
