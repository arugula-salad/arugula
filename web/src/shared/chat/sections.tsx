// The sidebar's sections and stars (#120), as the menus and buttons that
// change them: a room's menu (star, move to a section), a group's (sort,
// rename, delete), and the room header's star. The arrangement itself is
// ./sidebar's; it is kept by the store (`editSidebar`), per account.

import type { ChatStore } from "../chat-store";
import type { ChatRoom, ChatSort } from "../proto";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import { addSection, collapse, place, placeOf, removeSection, renameSection, setSort, sortOf, star, STARRED } from "./sidebar";

type Toast = (text: string) => void;
type At = { clientX: number; clientY: number; preventDefault(): void };

const refused = (toast: Toast) => (e: Error) => toast(`Couldn't keep that: ${e.message}`);

/** Ask for a name and make a section; `then` gets its id. */
export async function newSection(store: ChatStore, toast: Toast, then?: (id: string) => void) {
  const v = await askText("New section", "", "like Ops or Projects");
  const name = v?.trim();
  if (!name) return;
  const next = addSection(store.sidebar, name);
  const id = next.sections.at(-1)!.id;
  try {
    await store.editSidebar(() => next);
    then?.(id);
  } catch (e) {
    refused(toast)(e as Error);
  }
}

/** A room's menu: star it, move it to a section of your own, or put it
 * back where its kind puts it. */
export function roomMenuItems(store: ChatStore, r: ChatRoom, toast: Toast): MenuItem[] {
  const sb = store.sidebar;
  const at = placeOf(sb, r.id);
  const move = (to: string | null) => () => void store.editSidebar((s) => place(s, r.id, to)).catch(refused(toast));
  const items: MenuItem[] = [
    { label: at === STARRED ? "Unstar" : "Star", run: () => void store.editSidebar((s) => star(s, r.id, at !== STARRED)).catch(refused(toast)) },
    { header: "Move to" },
    ...sb.sections.map((x): MenuItem => ({ label: x.name, checked: at === x.id, run: move(x.id) })),
    {
      label: "New section…",
      run: () => void newSection(store, toast, (id) => void store.editSidebar((s) => place(s, r.id, id)).catch(refused(toast))),
    },
  ];
  if (at !== null) items.push("separator", { label: at === STARRED ? "Remove from Starred" : "Remove from its section", run: move(null) });
  return items;
}

export function openRoomMenu(e: At, store: ChatStore, r: ChatRoom, toast: Toast) {
  openMenu(e, roomMenuItems(store, r, toast));
}

/** A group's menu: how it sorts, and for a section of your own, rename
 * and delete. `fallback` is the sort a group has until it's chosen. */
export function openGroupMenu(e: At, store: ChatStore, g: { key: string; custom: boolean; title: string; fallback: ChatSort }, toast: Toast) {
  const sb = store.sidebar;
  const now = sortOf(sb, g.key, g.fallback);
  const sort = (to: ChatSort) => () => void store.editSidebar((s) => setSort(s, g.key, to)).catch(refused(toast));
  const items: MenuItem[] = [
    { header: "Sort" },
    { label: "By name", checked: now === "name", run: sort("name") },
    { label: "By recent activity", checked: now === "recent", run: sort("recent") },
  ];
  if (g.custom) {
    items.push(
      "separator",
      {
        label: "Rename…",
        run: async () => {
          const v = (await askText(`Rename ${g.title}`, g.title, "a name"))?.trim();
          if (v && v !== g.title) void store.editSidebar((s) => renameSection(s, g.key, v)).catch(refused(toast));
        },
      },
      { label: "Delete section", danger: true, hint: "Its rooms go back where they were", run: () => void store.editSidebar((s) => removeSection(s, g.key)).catch(refused(toast)) },
    );
  }
  openMenu(e, items);
}

/** Show or hide a group; remembered with the account's sidebar. */
export function toggleGroup(store: ChatStore, key: string, toast: Toast) {
  void store.editSidebar((s) => collapse(s, key, !s.collapsed.includes(key))).catch(refused(toast));
}

/** The room header's star, and a button for the room's menu (where the
 * phone moves a room: it has no drag). */
export function RoomStar({ store, room, toast }: { store: ChatStore; room: ChatRoom; toast: Toast }) {
  const on = placeOf(store.sidebar, room.id) === STARRED;
  return (
    <>
      <button
        class={on ? "chat-info on" : "chat-info"}
        title={on ? "Starred: it's at the top of your sidebar. Unstar" : "Star: keep it at the top of your sidebar"}
        aria-label={on ? "Unstar this room" : "Star this room"}
        aria-pressed={on}
        data-room-star
        onClick={() => void store.editSidebar((s) => star(s, room.id, !on)).catch(refused(toast))}
      >
        {on ? "★" : "☆"}
      </button>
      <button
        class="chat-info"
        title="Move to a section of your sidebar"
        aria-label="Move this room to a section"
        data-room-move
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openRoomMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, store, room, toast);
        }}
      >
        ⇆
      </button>
    </>
  );
}
