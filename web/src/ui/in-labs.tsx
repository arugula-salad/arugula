// Drawing a piece of Labs UI from the main bundle: the page asks for the
// Labs chunk when the machine has labs (`on`), and draws the piece once it is
// there. Once loaded it also keeps drawing for a page whose machine has none,
// so a huddle the person is in stays on screen when they look at another host.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { labsNow, loadLabs, onLabsLoaded, type Labs } from "../labs-load";

export function useLabs(on: boolean): Labs | null {
  const [, setTick] = useState(0);
  useEffect(() => {
    const off = onLabsLoaded(() => setTick((t) => t + 1));
    if (on)
      loadLabs().catch((e) => {
        console.error("Labs didn't load", e);
      });
    return off;
  }, [on]);
  return labsNow();
}

export function InLabs({ on, children }: { on: boolean; children: (labs: Labs) => ComponentChildren }) {
  const labs = useLabs(on);
  return labs ? <>{children(labs)}</> : null;
}
