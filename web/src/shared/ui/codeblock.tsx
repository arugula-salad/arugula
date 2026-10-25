// A chat message's fenced code (#109): the block, its language's colours
// (`highlight.ts`, fetched when the first block draws) and a button that
// copies the code, not the language label.

import { useEffect, useRef, useState } from "preact/hooks";
import { CopyButton } from "./copy";
import type { Piece } from "./highlight";

let loaded: typeof import("./highlight") | null = null;

export function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const [, again] = useState(0);
  const pre = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (loaded || !lang) return;
    void import("./highlight").then((m) => {
      loaded = m;
      again((n) => n + 1);
    }, () => {});
  }, [lang]);
  // Long blocks stay plain: scanning one is work for no one's benefit.
  const pieces: Piece[] | null = loaded && lang && code.length < 20_000 ? loaded.highlight(code, lang) : null;
  return (
    <div class="md-code" data-lang={lang || undefined}>
      <pre ref={pre}>{pieces ? pieces.map(([s, c], i) => (c ? <span key={i} class={c}>{s}</span> : s)) : code}</pre>
      <CopyButton text={code} select={() => pre.current} />
    </div>
  );
}
