// Composer (kit §4.1, 10): where you write to an agent or a room. It glows
// while you're in it; ↵ sends, ⇧↵ is a new line; the line under it says who
// it goes to, with any actions on what it writes to (Silence, Stop). A send
// that says it failed (`false`) keeps the text.

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { ArrowIcon } from "../ui/icons";
import "./kit.css";

export function Composer({
  placeholder,
  target,
  actions,
  disabled,
  onSend,
}: {
  placeholder: string;
  target?: string;
  actions?: ComponentChildren;
  disabled?: boolean;
  onSend: (text: string) => void | boolean | Promise<boolean | void>;
}) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const send = async () => {
    const t = text.trim();
    if (!t || busy || disabled) return;
    setBusy(true);
    try {
      if ((await onSend(t)) !== false) setText("");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="k-composer" data-kit="composer">
      <textarea
        rows={2}
        aria-label={placeholder}
        placeholder={placeholder}
        value={text}
        disabled={disabled}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
            e.preventDefault();
            void send();
          }
        }}
      />
      <div class="k-composer-foot">
        {actions}
        {target ? <span class="k-composer-to m-meta">{target}</span> : null}
        <button type="button" class="k-composer-send" aria-label="Send" title="Send" disabled={disabled || busy || !text.trim()} onClick={() => void send()}>
          <ArrowIcon dir="up" />
        </button>
      </div>
    </div>
  );
}
