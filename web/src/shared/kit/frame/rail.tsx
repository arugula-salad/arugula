// The rail (Lift 3, #1479; v6's): Arugula's mark, your projects' marks,
// the five places, then the Inbox (the one count across every project),
// Settings and you. Both entries draw it: the app's pages and a machine's
// own page each give their projects, places and account; it knows neither.
//
// Project marks (v6 decision 1): a stack of initials marks, the current one
// ringed, each with what's for you there; hovering one says where you left
// it. A click lands there; on the current one, it opens the project list.
// Past five projects, the current mark and the list.

import type { ComponentChildren } from "preact";
import { useRef } from "preact/hooks";
import { both, useBindings } from "../../keys";
import { MoreIcon } from "../../ui/icons";
import { initials } from "../avatar";
import "../kit.css";

export interface RailPlace {
  /** Its name in `data-open-<key>` and its `g` key's. */
  key: string;
  label: string;
  /** The button's title. */
  tip: string;
  icon: ComponentChildren;
  /** Lit: the place you're in. */
  here: boolean;
  /** An unread count on its corner (the Inbox's; Chat's). */
  count?: { n: number; mention: boolean };
  /** What a click does; with `href`, a plain click only. */
  onClick?: () => void;
  href?: string;
  target?: string;
}

export interface ProjectMark {
  id: string;
  name: string;
  here: boolean;
  /** What's for you there. */
  count: number;
  /** Where you left it, and its machine, for the hover. */
  left?: string;
  machine?: string;
  go: () => void;
}

/** Each place's `g` key (v6's). */
const GO: Record<string, string> = { overview: "o", boards: "b", docs: "d", machines: "m", chat: "c", inbox: "i" };

/** `g o`, `g b`…: the places by key, as a click on each (as last drawn). */
function useGoKeys(places: RailPlace[]) {
  const latest = useRef(places);
  latest.current = places;
  const keys = places.map((p) => p.key).filter((k) => GO[k]);
  useBindings(
    keys.map((key) => ({
      id: `go.${key}`,
      label: places.find((p) => p.key === key)!.label,
      group: "go" as const,
      context: "everywhere" as const,
      keys: both(`key:g key:${GO[key]}`),
      run: () => {
        const p = latest.current.find((x) => x.key === key);
        if (p?.onClick) p.onClick();
        else if (p?.href) location.assign(p.href);
      },
    })),
    [keys.join()],
  );
}

export function Rail({ brand, marks = [], onProjects, places, foot, me }: { brand: ComponentChildren; marks?: ProjectMark[]; onProjects?: () => void; places: RailPlace[]; foot: RailPlace[]; me?: ComponentChildren }) {
  useGoKeys([...places, ...foot]);
  return (
    <nav class="k-rail" aria-label="Places" data-kit="rail" data-rail>
      {brand}
      {marks.length > 0 && onProjects && <Marks marks={marks} onProjects={onProjects} />}
      {places.map((p) => (
        <RailButton key={p.key} place={p} />
      ))}
      <span class="k-rail-fill" />
      {foot.map((p) => (
        <RailButton key={p.key} place={p} />
      ))}
      {me}
    </nav>
  );
}

function Marks({ marks, onProjects }: { marks: ProjectMark[]; onProjects: () => void }) {
  // Past five, the current one and the list (v6 decision 1).
  const shown = marks.length > 5 ? marks.filter((m) => m.here) : marks;
  const here = shown.some((m) => m.here);
  return (
    <div class="k-marks" role="group" aria-label="Projects">
      {shown.map((m) => (
        <button
          key={m.id}
          type="button"
          class="k-mark-project"
          aria-current={m.here ? "true" : undefined}
          aria-label={`${m.name}${m.count ? `, ${m.count} for you` : ""}${m.here ? ": your projects" : ""}`}
          data-project-switcher={m.here ? "" : undefined}
          data-project-mark={m.id}
          onClick={m.here ? onProjects : m.go}
        >
          <span aria-hidden="true">{initials(m.name) || "?"}</span>
          {m.count > 0 && (
            <b class="k-rail-count" aria-hidden="true" data-count>
              {m.count}
            </b>
          )}
          <span class="k-mark-tip" aria-hidden="true">
            {m.name}
            <span class="m-meta">{[m.left ?? "Overview", m.machine, m.count ? `${m.count} for you` : ""].filter(Boolean).join(" · ")}</span>
          </span>
        </button>
      ))}
      {(shown.length < marks.length || !here) && (
        <button type="button" class="k-mark-project is-more" aria-label="Your projects" title="Your projects" data-project-switcher={here ? undefined : ""} onClick={onProjects}>
          <MoreIcon size="sm" />
        </button>
      )}
    </div>
  );
}

function RailButton({ place }: { place: RailPlace }) {
  const { key, label, tip, here, count, onClick, href, target, icon } = place;
  const inner = (
    <>
      <span class="k-rail-icon">{icon}</span>
      {count && count.n > 0 && (
        <b class={count.mention ? "k-rail-count is-mention" : "k-rail-count"} aria-hidden="true" data-count>
          {count.mention ? `@${count.n}` : count.n}
        </b>
      )}
      <span aria-hidden="true" data-rail-label>
        {label}
      </span>
    </>
  );
  const props = {
    class: "k-rail-place",
    "aria-label": count?.n ? `${label}, ${count.n}${count.mention ? ", for you" : ""}` : label,
    "aria-current": here ? ("page" as const) : undefined,
    title: tip,
    [`data-open-${key}`]: "",
  };
  if (href)
    return (
      <a
        {...props}
        href={href}
        target={target}
        onClick={(e) => {
          if (!onClick || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
          e.preventDefault();
          onClick();
        }}
      >
        {inner}
      </a>
    );
  return (
    <button type="button" {...props} onClick={onClick}>
      {inner}
    </button>
  );
}
