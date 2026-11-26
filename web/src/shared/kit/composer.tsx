// Composer (kit §4.1, 10): where you write to an agent or a room. It glows
// while you're in it; ↵ sends, ⇧↵ is a new line; the line under it says who
// it goes to.

import { useState } from "preact/hooks";
import { ArrowIcon } from "../ui/icons";
import "./kit.css";

export function Composer({ placeholder, target, onSend }: { placeholder: string; target?: string; onSend: (text: string) => void }) {
  const [text, setText] = useState("");
  const send = () => {
    if (!text.trim()) return;
    onSend(text);
    setText("");
  };
  return (
    <div class="k-composer" data-kit="composer">
      <textarea
        rows={2}
        aria-label={placeholder}
        placeholder={placeholder}
        value={text}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
            e.preventDefault();
            send();
          }
        }}
      />
      <div class="k-composer-foot">
        {target ? <span class="m-meta">{target}</span> : null}
        <button type="button" class="k-composer-send" aria-label="Send" title="Send" onClick={send}>
          <ArrowIcon dir="up" />
        </button>
      </div>
    </div>
  );
}
