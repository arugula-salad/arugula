// A Doc's layout as a row's mark in a Docs list (Arugula #830): a small icon
// before the title for a tabs or a prototype Doc, and nothing for a plain one.

import { PrototypeIcon, TabsIcon } from "../ui/icons";
import { docLayout } from "./layout";

export function LayoutMark({ layout }: { layout: string | null | undefined }) {
  const l = docLayout(layout);
  if (l === "doc") return null;
  const what = l === "tabs" ? "Tabs Doc" : "Prototype Doc";
  return (
    <span class="doc-layout-mark" role="img" aria-label={what} title={what} data-doc-layout={l}>
      {l === "tabs" ? <TabsIcon /> : <PrototypeIcon />}
    </span>
  );
}
