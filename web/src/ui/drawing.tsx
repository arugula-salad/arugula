// A drawing in a Doc (#964): SVG text the agent wrote, shown only through
// <img>. An image runs no script and loads nothing outside, so there is
// nothing to sanitise and the text never becomes DOM: never inline it
// (no dangerouslySetInnerHTML, no <svg> built from it).

import { useMemo, useState } from "preact/hooks";
import { checkSvg, svgUrl } from "./embeds";

export function Drawing({ svg, alt }: { svg: string; alt: string }) {
  const url = useMemo(() => svgUrl(svg), [svg]);
  // The browser can't read an SVG that isn't well-formed XML: the <img>
  // says so, and we show why instead of a broken picture.
  const [broken, setBroken] = useState<string | null>(null);
  const why = checkSvg(svg) ?? broken;
  if (why) {
    return (
      <div class="chart-note" data-drawing-note>
        <p class="dim">Drawing not drawn: {why.replace(/^svg: /, "")}</p>
        <pre>{svg}</pre>
      </div>
    );
  }
  return <img class="doc-drawing" src={url} alt={alt || "drawing"} onError={() => setBroken("the browser couldn't read it as an SVG image")} />;
}
