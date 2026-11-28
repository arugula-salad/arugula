// Composer (kit §4.1, 10): where you write to an agent or a room. It glows
// while you're in it; ↵ sends, ⇧↵ is a new line; the line under it says who
// it goes to, with any actions on what it writes to (Silence, Stop). A send
// that says it failed (`false`) keeps the text. Chat's composer takes this
// look in the agent panel (RoomBody's `target`), keeping its mentions,
// files and drafts. With `files`, it takes files as chat's does (#1557):
// dropped on it (outlined while a drag is over it), pasted, or picked with
// 📎, and they go with the next send.

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { AttachButton, attachKey, AttachChips, dropTarget, filesOf } from "../chat/files-ui";
import { MAX_FILES, pastedName } from "../chat/files";
import { ArrowIcon } from "../ui/icons";
import "./kit.css";

export function Composer({
  placeholder,
  target,
  actions,
  disabled,
  files: takes,
  onSend,
}: {
  placeholder: string;
  target?: string;
  actions?: ComponentChildren;
  disabled?: boolean;
  /** It takes files. */
  files?: boolean;
  onSend: (text: string, files: File[]) => void | boolean | Promise<boolean | void>;
}) {
  const [text, setText] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const [over, setOver] = useState(false);
  const [busy, setBusy] = useState(false);
  const add = (more: File[]) => setFiles((xs) => [...xs, ...more].slice(0, MAX_FILES));
  const send = async () => {
    const t = text.trim();
    if ((!t && !files.length) || busy || disabled) return;
    setBusy(true);
    try {
      if ((await onSend(t, files)) !== false) {
        setText("");
        setFiles([]);
      }
    } finally {
      setBusy(false);
    }
  };
  const drop = takes && !disabled && { ...dropTarget(setOver, add), onKeyDown: attachKey };
  const chips = files.map((f, i) => ({ key: String(i), name: pastedName(f), size: f.size, progress: 0, state: busy ? ("uploading" as const) : ("done" as const) }));
  return (
    <div class={`k-composer${over ? " dropping" : ""}`} data-kit="composer" {...(drop || {})}>
      {takes && <AttachChips items={chips} remove={(k) => setFiles((xs) => xs.filter((_, i) => String(i) !== k))} />}
      <textarea
        class="k-composer-field"
        rows={2}
        aria-label={placeholder}
        placeholder={placeholder}
        value={text}
        disabled={disabled}
        onInput={(e) => setText(e.currentTarget.value)}
        onPaste={(e) => {
          // A screenshot on the clipboard is a file; text pastes as text.
          const pasted = takes ? filesOf(e.clipboardData) : [];
          if (!pasted.length) return;
          e.preventDefault();
          add(pasted);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
            e.preventDefault();
            void send();
          }
        }}
      />
      <div class="k-composer-foot">
        {takes && !disabled && <AttachButton add={add} />}
        {actions}
        {target ? <span class="k-composer-to m-meta">{target}</span> : null}
        <button type="button" class="k-composer-send" aria-label="Send" title="Send" disabled={disabled || busy || (!text.trim() && !files.length)} onClick={() => void send()}>
          <ArrowIcon dir="up" />
        </button>
      </div>
    </div>
  );
}
