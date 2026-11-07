// The fixture page for the shared menu (#757): a menu with icons, a facet
// menu whose ticks stay open, and a side sheet with a menu opened from inside
// it, a popover (arbitrary content in the menu's box), over a page with a counter under where the menu's backdrop sits (a tap
// on the backdrop must not reach it). menus.spec.ts builds this with Vite and
// serves it, so no daemon is involved.

import { render } from "preact";
import { useRef, useState } from "preact/hooks";
import "../src/shared/style.css";
import "../src/shared/buttons.css";
import { Svg } from "../src/shared/ui/icons";
import {
  MenuLayer,
  menuBelow,
  openMenu,
  Popover,
  pressedShut,
  type MenuItem,
} from "../src/shared/ui/menu";
import { SideSheet } from "../src/shared/ui/side-sheet";

const dot = (
  <Svg size={16}>
    <circle cx="12" cy="12" r="6" />
  </Svg>
);

function Page() {
  const [ran, setRan] = useState("");
  const [facet, setFacet] = useState<string[]>([]);
  const [sheet, setSheet] = useState(false);
  const [pop, setPop] = useState(false);
  const popButton = useRef<HTMLButtonElement>(null);
  const [under, setUnder] = useState(0);
  const menuButton = useRef<HTMLButtonElement>(null);
  const facetButton = useRef<HTMLButtonElement>(null);
  const inSheet = useRef<HTMLButtonElement>(null);
  const items: MenuItem[] = [
    {
      label: "Copy link",
      icon: dot,
      run: () => setRan("copy"),
      attrs: { "data-item": "copy" },
    },
    {
      label: "Rename",
      icon: dot,
      shortcut: "F2",
      run: () => setRan("rename"),
      attrs: { "data-item": "rename" },
    },
    // Two lines together and one at the end, as a menu built from parts
    // that can be empty draws them: the menu keeps one between rows.
    "separator",
    "separator",
    {
      label: "Move",
      submenu: [
        { label: "Left", run: () => setRan("left"), attrs: { "data-item": "left" } },
        { label: "Right", run: () => setRan("right"), attrs: { "data-item": "right" } },
      ],
    },
    {
      label: "Delete",
      icon: dot,
      danger: true,
      run: () => setRan("delete"),
      attrs: { "data-item": "delete" },
    },
    "separator",
  ];
  const facets: MenuItem[] = ["docs", "ui", "boards"].map((v) => ({
    label: v,
    detail: "2",
    checked: facet.includes(v),
    multi: true,
    run: () =>
      setFacet((f) => (f.includes(v) ? f.filter((x) => x !== v) : [...f, v])),
    attrs: { "data-facet-value": v },
  }));
  const at =
    (
      b: { current: HTMLButtonElement | null },
      list: MenuItem[],
      name: string,
    ) =>
    () =>
      b.current &&
      openMenu(menuBelow(b.current), list, {
        from: b.current,
        label: name,
        attrs: { "data-menu-name": name },
      });
  return (
    <main style="padding:24px">
      <button
        type="button"
        class="secondary"
        ref={menuButton}
        data-open="menu"
        onClick={at(menuButton, items, "Actions")}
      >
        Actions
      </button>
      <button
        type="button"
        class="secondary"
        ref={facetButton}
        data-open="facet"
        onClick={at(facetButton, facets, "Label")}
      >
        Label{facet.length > 0 && ` · ${facet.length}`}
      </button>
      <button
        type="button"
        class="secondary"
        data-open="sheet"
        onClick={() => setSheet(true)}
      >
        Open sheet
      </button>
      <button
        type="button"
        class="secondary"
        ref={popButton}
        data-open="popover"
        aria-haspopup="dialog"
        onClick={() => pressedShut(popButton.current) || setPop(!pop)}
      >
        Details
      </button>
      {pop && (
        <Popover
          from={popButton}
          label="Details"
          attrs={{ "data-popover": true }}
          onClose={() => setPop(false)}
        >
          <div style="padding:12px">
            <input aria-label="Name" data-autofocus data-pop="name" />
            <a href="#top" data-pop="link">
              A link
            </a>
            <button type="button" data-pop="ok" onClick={() => setPop(false)}>
              OK
            </button>
          </div>
        </Popover>
      )}
      <p data-ran>{ran}</p>
      {/* Under the phone's backdrop, bottom left, where a tap lands if it falls through. */}
      <button
        type="button"
        data-under
        style="position:fixed;left:0;bottom:0;width:120px;height:60px"
        onClick={() => setUnder(under + 1)}
      >
        under {under}
      </button>
      {sheet && (
        <SideSheet
          title="A row"
          close={() => setSheet(false)}
          root={{ "data-sheet": true }}
        >
          <button
            type="button"
            class="secondary"
            ref={inSheet}
            data-open="sheet-menu"
            onClick={at(inSheet, items, "Row actions")}
          >
            Row actions
          </button>
        </SideSheet>
      )}
      <MenuLayer />
    </main>
  );
}

render(<Page />, document.getElementById("root")!);
