// "Edit in nvim": a checkbox for the Editor preference (Settings › Editor),
// shown where people pick or meet an editor. Ticked is nvim; unticked is
// what it was before (VS Code, or Ask each time). `nvim`: whether the
// daemon found nvim on its PATH (`/api/setup`), when the caller knows; it
// says there's nothing to install only then. `titled`: a heading above
// already says "Edit in nvim" (Getting started's card), so the label is
// what it does.

import { useEffect, useState } from "preact/hooks";
import { subscribeSeen } from "../../shared/seen";
import { editsInNvim, setEditInNvim } from "../open-in-editor";

export function EditInNvim({
  disabled,
  title,
  onChange,
  nvim,
  titled,
}: {
  disabled?: boolean;
  title?: string;
  onChange?: (on: boolean) => void;
  nvim?: boolean;
  titled?: boolean;
}) {
  const [, tick] = useState(0);
  const what = "Opens files in nvim, in a pane.";
  const needs = nvim === true ? " Nothing to install." : nvim === false ? " Needs nvim, which isn't on this machine's PATH yet." : "";
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
      {titled ? (
        <span data-edit-in-nvim-says>
          {what}
          {needs}
        </span>
      ) : (
        <span>
          Edit in nvim{" "}
          <small data-edit-in-nvim-says>
            {what}
            {needs}
          </small>
        </span>
      )}
    </label>
  );
}
