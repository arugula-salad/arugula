// A project Doc on its own (#1258): `/embed/p/:project/docs/:doc` on
// control's site, for the native app's web view. Under `/embed`, a hub path
// the platform page's host never answers, so it's no page of the platform's.
// Only the Doc, with its comments in the margin (or under it on a phone): no rail, header,
// onboarding or first-run dialog. The app puts its session's cookie in the
// web view, and that session stays a native one: the page reads through the
// routes control lists for `native` (the Doc, the reader, the Doc's thread)
// and posts comments there, and nothing else.
//
// Links go up to the app: a tap on a board, an issue, a thread or another
// Doc is posted to `window.webkit.messageHandlers.arugula` as an intent
// (`EmbedIntent`), which the app opens natively. In a browser, with no app
// around it, a link opens on the platform's page as usual.
//
// In the shell's build, as the shared Doc's page (doc-view.tsx) is: it draws
// the Doc with the block's renderer and shares nothing of the platform's.

import { render } from "preact";
import { useEffect, useMemo, useState } from "preact/hooks";
import "./doc-embed.css";
import type { PlatformProjects, ProjectDocView } from "../shared/proto";
import { chat } from "../shared/chat-store";
import { DocBlock } from "../shared/docs/doc-render";
import { DocComments } from "../shared/docs/doc-comments";
import { reachOf } from "../shared/docs/doc-reach";
import { DocError, projectDocs } from "../shared/docs/store";
import { stateOf, useProjectImages } from "../shared/docs/project-docs";
import { whoLabel } from "../shared/issues/text";
import { setProtoOrigin } from "../shared/ui/prototype-frame";

/** What the page tells the app: a link to open natively, or that it can't
 * read the Doc (`signed_out`: the app's session was refused). */
export type EmbedIntent =
  | { kind: "doc" | "board" | "issue" | "chat" | "dm" | "run" | "session"; project: string; id: string; url: string }
  | { kind: "link"; url: string }
  | { kind: "ready"; title: string }
  | { kind: "signed_out" };

interface Shell {
  postMessage(m: EmbedIntent): void;
}

/** The app around the page, when there is one. */
const shell = (): Shell | null => (window as { webkit?: { messageHandlers?: { arugula?: Shell } } }).webkit?.messageHandlers?.arugula ?? null;

const tell = (m: EmbedIntent) => {
  try {
    shell()?.postMessage(m);
  } catch {
    // An app that went away: nothing to tell.
  }
};

const KINDS: Record<string, Exclude<EmbedIntent, { kind: "link" | "ready" | "signed_out" }>["kind"]> = {
  docs: "doc",
  boards: "board",
  issues: "issue",
  chat: "chat",
  dm: "dm",
  runs: "run",
  sessions: "session",
};

/** What a link on the platform's page is, as the app opens it: the
 * project's one thing (`/p/:project/<kind>/:id`), else a link. */
export function intentOf(url: URL): EmbedIntent {
  const parts = url.pathname.split("/").filter(Boolean);
  const kind = parts.length === 4 && parts[0] === "p" ? KINDS[parts[2]!] : undefined;
  // A slug after an id (`prj_…-arugula`) is for people.
  const id = (s: string) => s.replace(/^((?:[a-z]+_)?[0-9a-f]+)-.*$/, "$1");
  return kind ? { kind, project: id(parts[1]!), id: id(parts[3]!), url: url.href } : { kind: "link", url: url.href };
}

/** The Doc's address: `/embed/p/:project/docs/:doc`, a slug after either id. */
function where(): { project: string; doc: string } | null {
  const m = /^\/embed\/p\/(prj_[0-9a-f]+)(?:-[^/]*)?\/docs\/(pdc_[0-9a-f]+)(?:-[^/]*)?\/?$/.exec(location.pathname);
  return m ? { project: m[1]!, doc: m[2]! } : null;
}

/** Where the platform's page is (`/control.json`), for a comment's link and a
 * link the app doesn't take; this page's own origin without one. */
let platform = location.origin;

type View = { kind: "loading" } | { kind: "missing"; why: string } | { kind: "signed_out" } | { kind: "doc"; view: ProjectDocView };

function Page({ project, doc }: { project: string; doc: string }) {
  const [shown, setShown] = useState<View>({ kind: "loading" });
  const [names, setNames] = useState<Map<string, string>>(new Map());
  useEffect(() => {
    let seen = 0;
    const again = () => {
      const n = ++seen;
      projectDocs.get(project, doc).then(
        (view) => {
          if (n !== seen) return;
          setShown({ kind: "doc", view });
          document.title = view.doc.title || "Untitled";
        },
        (e: Error) => {
          if (n !== seen) return;
          if (e instanceof DocError && e.status === 401) {
            tell({ kind: "signed_out" });
            return setShown({ kind: "signed_out" });
          }
          setShown({ kind: "missing", why: e instanceof DocError && e.status === 404 ? "it isn't there, or isn't yours to read" : e.message });
        },
      );
    };
    again();
    const off = chat.onObject(doc, again);
    const back = () => document.visibilityState === "visible" && again();
    document.addEventListener("visibilitychange", back);
    return () => {
      off();
      document.removeEventListener("visibilitychange", back);
    };
  }, [project, doc]);
  // The reader, for chat, and the project's people, for who made it.
  useEffect(() => {
    void fetch("/api/projects", { credentials: "same-origin" })
      .then((r) => (r.ok ? (r.json() as Promise<PlatformProjects>) : null))
      .then((v) => {
        if (!v) return;
        const roster = v.projects.flatMap((p) => p.team?.members.filter((m) => m.account === v.me.account).map((m) => m.name) ?? []);
        chat.start(v.me.account, [v.me.login, v.me.name ?? "", ...roster]);
        const p = v.projects.find((x) => x.id === project);
        setNames(new Map([[v.me.account, "you"], ...(p?.team?.members.map((m) => [m.account, m.name] as [string, string]) ?? [])]));
      })
      .catch(() => {});
  }, [project]);
  const view = shown.kind === "doc" ? shown.view : null;
  const state = useMemo(() => (view ? stateOf(view) : null), [view]);
  const images = useProjectImages(project, doc);
  const headings = useMemo(() => new Map(view?.sections.map((s) => [s.id, s.heading]) ?? []), [view]);
  useEffect(() => {
    if (view) tell({ kind: "ready", title: view.doc.title });
  }, [view?.doc.id]);
  if (shown.kind === "loading") return <p class="doc-loading">Loading the Doc…</p>;
  if (shown.kind === "signed_out")
    return (
      <div class="doc-notice" data-embed-signed-out>
        <p class="doc-notice-title">Sign in to read this Doc.</p>
      </div>
    );
  if (shown.kind === "missing")
    return (
      <div class="doc-notice" data-doc-missing>
        <p class="doc-notice-title">That Doc can't be read: {shown.why}.</p>
      </div>
    );
  const reach = reachOf(shown.view.doc);
  return (
    <main class="project-doc doc-embed" data-doc-page={doc} data-embed>
      <DocComments doc={doc} url={`${platform}/p/${project}/docs/${doc}`} headings={headings} mention={reach.mention} reaches={reach.label}>
        <DocBlock
          id={0}
          docKey={doc}
          s={state}
          fresh={NO_FRESH}
          load={images.load}
          cached={images.cached}
          link="page"
          byline={<span data-doc-maker>{whoLabel(shown.view.doc.author, shown.view.doc.agent, (a) => names.get(a))}</span>}
        />
      </DocComments>
    </main>
  );
}
const NO_FRESH = new Map<string, number>();

/** A tap on a link: the app's to open, when there's an app. */
function follow(e: MouseEvent) {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const a = (e.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
  if (!a) return;
  const raw = a.getAttribute("href")!;
  // A section of this Doc (`#s2`) stays here.
  if (raw.startsWith("#")) return;
  const url = new URL(raw, platform);
  // The platform's paths aren't this site's: a link to one goes there.
  if (url.origin === location.origin) url.href = new URL(url.pathname + url.search + url.hash, platform).href;
  e.preventDefault();
  if (shell()) return tell(intentOf(url));
  window.open(url.href, "_blank", "noopener");
}

async function boot() {
  const root = document.getElementById("root")!;
  const at = where();
  // The Doc's own look follows the reader's system, as the shared page's does.
  const light = matchMedia("(prefers-color-scheme: light)");
  const scheme = () => (document.documentElement.dataset.scheme = light.matches ? "light" : "dark");
  scheme();
  light.addEventListener("change", scheme);
  if (!at) return render(<p class="doc-notice-title">That isn't a Doc's address.</p>, root);
  // Where prototypes run (#506) and where the platform's page is.
  const info = (await fetch("/control.json")
    .then((r) => (r.ok ? r.json() : null))
    .catch(() => null)) as { proto?: string | null; platform?: string | null } | null;
  setProtoOrigin(info?.proto && /^https?:\/\/[^/]+$/.test(info.proto) ? info.proto : null);
  if (info?.platform && /^https?:\/\/[^/]+$/.test(info.platform)) platform = info.platform;
  document.addEventListener("click", follow);
  render(<Page project={at.project} doc={at.doc} />, root);
}

void boot();
