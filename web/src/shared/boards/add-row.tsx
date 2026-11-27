// Add row (Lift 5a, plan §3.3): a Sheet with a title, a description and
// Start an agent on it. It opens a native issue in the project, puts it on
// the board as a row (when there's a board), and asks for an agent on it
// (the issue page's Agent on this) when that's on. With no board it is New
// issue: All issues' and the project's own.

import { useState } from "preact/hooks";
import type { NativeIssue } from "../proto";
import { usePlatformApi } from "../platform-api";
import { issues } from "../issues/store";
import { Button } from "../kit/button";
import { TextField, Toggle } from "../kit/field";
import { Sheet } from "../kit/sheet";
import { boards } from "./store";

export function AddRowSheet({ project, board, close, done }: { project: string; board?: string; close: () => void; done: (issue: NativeIssue) => void }) {
  const { toast } = usePlatformApi();
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [agent, setAgent] = useState(false);
  const [busy, setBusy] = useState(false);
  const add = async () => {
    if (!title.trim() || busy) return;
    setBusy(true);
    let made: NativeIssue;
    try {
      made = await issues.create(project, { title: title.trim(), body, labels: [], assignees: [] });
    } catch (e) {
      toast(`Couldn't open the issue: ${(e as Error).message}`);
      return setBusy(false);
    }
    if (board) await boards.put(board, { entry: "row", pointer: { kind: "issue", id: made.id } }).catch((e: Error) => toast(`Opened #${made.number} but couldn't put it on the board: ${e.message}`));
    if (agent) await issues.agent(made.id).catch((e: Error) => toast(`Opened #${made.number} but couldn't start an agent: ${e.message}`));
    done(made);
  };
  return (
    <Sheet title={board ? "Add row" : "New issue"} onClose={close}>
      <form
        class="add-row-form"
        data-add-row
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <TextField label="Title" value={title} onInput={setTitle} autoFocus data-add-row-title />
        <TextField label="Description" value={body} onInput={setBody} lines={6} placeholder="What's it about?" data-add-row-body />
        <Toggle label="Start an agent on it" on={agent} onChange={setAgent} />
        <div class="k-row-actions">
          <Button onClick={close}>Cancel</Button>
          <Button kind="primary" type="submit" disabled={busy || !title.trim()} data-add-row-add>
            {board ? "Add row" : "Open issue"}
          </Button>
        </div>
      </form>
    </Sheet>
  );
}
