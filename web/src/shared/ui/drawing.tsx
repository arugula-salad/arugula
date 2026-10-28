// A drawing in a Doc (#964): SVG text the agent wrote, shown only through
// <img>. An image runs no script and loads nothing outside, so there is
// nothing to sanitise and the text never becomes DOM: never inline it
// (no dangerouslySetInnerHTML, no <svg> built from it).
//
// A drawing may name the Doc's colours (#256, `fill="leaf-soft"`): CSS can't
// reach into an <img>, so the names become the current scheme's values in
// the text before it is made a URL, and again when the scheme changes. One
// that names them sits on the page's paper; one in raw hex keeps the light
// backing it was drawn for.

import { useEffect, useMemo, useState } from "preact/hooks";
import { resolve, usesNames, type Scheme } from "../docs/colours";
import { checkSvg, svgUrl } from "./embeds";

const scheme = (): Scheme => (document.documentElement.dataset.scheme === "light" ? "light" : "dark");

/** `html[data-scheme]`, as the app's appearance or the share viewer sets it. */
function useScheme(): Scheme {
  const [s, set] = useState(scheme);
  useEffect(() => {
    const mo = new MutationObserver(() => set(scheme()));
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-scheme"] });
    return () => mo.disconnect();
  }, []);
  return s;
}

export function Drawing({ svg, alt }: { svg: string; alt: string }) {
  const s = useScheme();
  const named = useMemo(() => usesNames(svg), [svg]);
  const url = useMemo(() => svgUrl(named ? resolve(svg, s) : svg), [svg, named, s]);
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
  return (
    <img
      class="doc-drawing"
      data-named={named ? "" : undefined}
      src={url}
      alt={alt || "drawing"}
      onError={() => setBroken("the browser couldn't read it as an SVG image")}
    />
  );
}
