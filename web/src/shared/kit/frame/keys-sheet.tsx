// The `?` sheet (Lift 3, #1479): every key this page has, in three columns,
// drawn from the one table (shared/keys.ts), so a binding can't be missing
// from it. `?` opens and closes it; so does the footer's `?`.

import { bindings, both, GROUPS, hintOf, subscribeKeys, useBinding, useEscape } from "../../keys";
import { useSubscribe } from "../../ui/hooks";
import { Kbd } from "../kbd";
import { Sheet } from "../sheet";
import "../kit.css";

let open = false;
const listeners = new Set<() => void>();
const set = (to: boolean) => {
  open = to;
  listeners.forEach((fn) => fn());
};

/** Open the sheet (the footer's `?`). */
export const openKeys = () => set(true);

/** The sheet, and its key; once per page. */
export function KeysSheetLayer() {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  useSubscribe(subscribeKeys);
  useBinding({ id: "keys", label: "Every shortcut", group: "app", keys: both("key:?"), run: () => set(!open) }, []);
  // Focus stays where it was (the sheet has nothing to focus): Escape is the table's.
  useEscape("keys.close", open ? () => set(false) : null, [open]);
  if (!open) return null;
  const all = bindings();
  return (
    <Sheet title="Keyboard shortcuts" onClose={() => set(false)}>
      <div class="k-keys" data-keys-sheet>
        {GROUPS.map((g) => {
          const list = all.filter((b) => (b.group ?? "screen") === g.id);
          return list.length ? (
            <section key={g.id} class="k-keys-group" aria-label={g.label}>
              <h3 class="m-meta">{g.label}</h3>
              {list.map((b) => (
                <p key={b.id} class="k-keys-one" data-binding={b.id}>
                  <span class="k-keys-chord">
                    <Kbd keys={hintOf(b) ?? ""} />
                  </span>
                  {b.label}
                </p>
              ))}
            </section>
          ) : null;
        })}
      </div>
    </Sheet>
  );
}
