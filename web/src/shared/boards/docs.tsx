// A board's Docs (#517), wherever the board is drawn: the strip under its
// goal, and the Docs section of a row's drawer. A project Doc opens its page
// on the platform page; a machine's Doc opens in the shell when the shell is
// on that machine (`PlatformApi.machineDoc`), and elsewhere says which
// machine has it. Writers link a project Doc from the picker and take one
// off; machine Docs come in through agents (`board_write` `link_doc`).

import { useEffect, useState } from "preact/hooks";
import type { BoardPointer, ProjectDoc } from "../proto";
import { usePlatformApi } from "../platform-api";
import { boards } from "./store";
import { docHref, docWords } from "./text";

/** What a Doc's chip needs: a strip entry has all of it, a row's reference has the pointer and its `doc`. */
export interface DocShown {
  pointer: BoardPointer;
  title: string;
  sections?: number;
  written?: number;
  missing?: boolean;
  machine_name?: string;
}

/** A Doc as a chip: its title as a link where it opens, and `3/5` written or the machine it's on. A project Doc that was deleted is plain text. `remove` is the ×, for those who may write. */
export function DocLink({ d, project, platform, remove }: { d: DocShown; project: string; platform: (path: string) => string | undefined; remove?: () => void }) {
  const api = usePlatformApi();
  const p = d.pointer;
  const words = docWords(d);
  let name;
  let tail = words;
  if (p.kind === "doc" && !d.missing) {
    const href = docHref(project, p.id, platform);
    name = href ? (
      <a class="board-ptr" href={href} data-board-doc-open>
        {d.title}
      </a>
    ) : (
      <span class="board-ptr">{d.title}</span>
    );
  } else if (p.kind === "machine_doc") {
    const machine = p.machine;
    const here = api.machineDoc?.here(machine);
    const control = api.controlHref?.(`machine=${machine}`);
    if (here) {
      name = (
        <a class="board-ptr" href="#" data-board-doc-open onClick={(e) => (e.preventDefault(), api.machineDoc!.open(machine, p.doc))}>
          {d.title}
        </a>
      );
    } else if (control) {
      name = (
        <a class="board-ptr" href={control} target="_blank" rel="noopener noreferrer" title="Open its machine in control" data-board-doc-open>
          {d.title}
        </a>
      );
    } else {
      name = <span class="board-ptr">{d.title}</span>;
      // The shell, on another machine: where it is, not a way to open it from here.
      if (api.machineDoc) tail = `${words}, open it there`;
    }
  } else {
    name = <span class="board-ptr dim">{d.title}</span>;
  }
  return (
    <span class="sw-chip board-chip board-doc" data-board-doc={p.kind === "doc" || p.kind === "machine_doc" ? (p.kind === "doc" ? p.id : p.doc) : undefined} data-board-doc-kind={p.kind} data-board-doc-missing={d.missing ? "" : undefined}>
      {name}
      {tail && <span class="dim board-doc-words"> {tail}</span>}
      {remove && (
        <button type="button" class="board-more board-doc-remove" aria-label={`Take ${d.title} off`} data-board-doc-remove onClick={remove}>
          ×
        </button>
      )}
    </span>
  );
}

/** "+ Doc": the project's Docs by title, with a filter box, for a writer to pick one. `taken` are the `pdc_` ids already there. */
export function DocPicker({ project, taken, pick, close }: { project: string; taken: Set<string>; pick: (p: BoardPointer) => Promise<unknown>; close: () => void }) {
  const [docs, setDocs] = useState<ProjectDoc[] | null>(null);
  const [err, setErr] = useState("");
  const [q, setQ] = useState("");
  useEffect(() => {
    let live = true;
    boards.docs(project).then(
      (l) => live && setDocs(l.docs),
      (e: Error) => live && setErr(e.message),
    );
    return () => void (live = false);
  }, [project]);
  const left = (docs ?? []).filter((d) => !taken.has(d.id));
  const shown = left.filter((d) => d.title.toLowerCase().includes(q.trim().toLowerCase()));
  return (
    <div class="board-doc-picker" role="dialog" aria-label="Link a Doc" data-board-doc-picker>
      <input type="text" value={q} placeholder="Filter the project's Docs" aria-label="Filter the project's Docs" data-board-doc-filter autoFocus onInput={(e) => setQ(e.currentTarget.value)} />
      {err && <p class="dim" data-board-doc-error>Couldn't read the project's Docs: {err}</p>}
      {!err && docs === null && <p class="dim">Loading…</p>}
      {docs && left.length === 0 && (
        <p class="dim" data-board-doc-empty>
          No Docs in this project to link. Agents link their Docs with board_write.
        </p>
      )}
      {docs && left.length > 0 && shown.length === 0 && <p class="dim">No Doc matches.</p>}
      <ul class="board-doc-list">
        {shown.map((d) => (
          <li key={d.id}>
            <button type="button" data-board-doc-pick={d.id} onClick={() => void pick({ kind: "doc", id: d.id }).then(close)}>
              {d.title} <span class="dim">{docWords({ pointer: { kind: "doc", id: d.id }, sections: d.sections, written: d.written })}</span>
            </button>
          </li>
        ))}
      </ul>
      <button type="button" class="secondary small" data-board-doc-picker-close onClick={close}>
        Cancel
      </button>
    </div>
  );
}
