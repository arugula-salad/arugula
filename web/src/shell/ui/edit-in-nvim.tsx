// "Edit in nvim": a checkbox for the Editor preference (Settings › Editor),
// shown where people pick or meet an editor. Ticked is nvim; unticked is
// what it was before (VS Code, or Ask each time).

import { useEffect, useState } from "preact/hooks";
import { subscribeSeen } from "../../shared/seen";
import { editsInNvim, setEditInNvim } from "../open-in-editor";

export function EditInNvim({ disabled, title, onChange }: { disabled?: boolean; title?: string; onChange?: (on: boolean) => void }) {
  const [, tick] = useState(0);
  useEffect(() => subscribeSeen(() => tick((t) => t + 1)), []);
  return (
    <label class="check edit-in-nvim" title={title}>
      <input
        type="checkbox"
        data-edit-in-nvim
        checked={disabled || editsInNvim()}
        disabled={disabled}
        onChange={(e) => {
          setEditInNvim(e.currentTarget.checked);
          onChange?.(e.currentTarget.checked);
        }}
      />
      <span>
        Edit in nvim <small>Opens files in nvim, in a pane. Nothing to install.</small>
      </span>
    </label>
  );
}
