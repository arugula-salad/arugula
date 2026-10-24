// "alecraso is typing…" above the composer (#111). The store keeps who
// pinged in the last few seconds (`chat.typingIn`); this says it, and
// redraws when the oldest name's six seconds are up.

import { useEffect, useState } from "preact/hooks";
import { chat } from "../chat-store";
import { useSubscribe } from "../ui/hooks";

/** Up to three names, then "several people". */
export function typingText(names: string[]): string {
  if (!names.length) return "";
  if (names.length > 3) return "Several people are typing…";
  if (names.length === 1) return `${names[0]} is typing…`;
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]} are typing…`;
}

export function TypingLine({ room }: { room: string }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const [, tick] = useState(0);
  const ends = chat.typingEnds(room);
  useEffect(() => {
    if (ends === null) return;
    const t = window.setTimeout(() => tick((n) => n + 1), Math.max(0, ends - Date.now()) + 20);
    return () => clearTimeout(t);
  }, [ends]);
  const text = typingText(chat.typingIn(room));
  // Always there, so the box doesn't jump when a name comes.
  return (
    <p class="typing-line" role="status" aria-live="polite" data-typing={text ? "" : undefined}>
      {text}
    </p>
  );
}
