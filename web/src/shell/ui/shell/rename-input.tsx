// Renaming a tab or a session in place.

import { useLayoutEffect, useRef, useState } from "preact/hooks";

/** What's being renamed, if anything. */
export type Renaming = { kind: "tab" | "session"; id: number } | null;

export function RenameInput({ value: initial, onDone }: { value: string; onDone: (v: string | null) => void }) {
  // What's typed, so a re-render (live pane updates arrive every second)
  // doesn't put the old name back.
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useLayoutEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const finish = (v: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(v === null ? null : v.trim());
  };
  return (
    <input
      ref={ref}
      class="rename"
      value={value}
      onInput={(e) => setValue((e.target as HTMLInputElement).value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") finish((e.target as HTMLInputElement).value);
        if (e.key === "Escape") finish(null);
      }}
      onBlur={(e) => finish((e.target as HTMLInputElement).value)}
    />
  );
}
