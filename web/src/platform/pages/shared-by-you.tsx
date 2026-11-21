// Shared by you (Arugula #1315), at the foot of My Docs: every Doc this
// account has shared with a link, as control's Shared Docs dialog lists
// them (shell/ui/shared-docs.tsx). The hub lists them (../doc-links.ts); the
// machine that shared one knows its title and its link, key and all, so
// Open and Copy link ask it, through the page's own way to it
// (`PlatformApi.docLinks`), and wait while it's offline. Unshare asks it
// too (it clears the Doc's block), else takes the link down at the hub.
// Nothing shows until the hub answers here, or when there's no link.

import { useEffect, useRef, useState } from "preact/hooks";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { useSubscribe } from "../../shared/ui/hooks";
import { ListItem, ListSection } from "../../shared/ui/list-page";
import { usePlatformApi } from "../../shared/platform-api";
import { Time } from "../../shared/ui/time";
import { docLinks, type DocLink } from "../doc-links";
import { toast } from "../toast";

export const OFFLINE = "its machine is offline";

const size = (b: number) => (b < 1 << 20 ? `${Math.max(1, Math.round(b / 1024))} KB` : `${(b / (1 << 20)).toFixed(1)} MB`);
const opened = (n: number) => (n === 0 ? "not opened yet" : n === 1 ? "opened about once" : `opened about ${n} times`);

export function SharedByYou({ focus }: { focus?: boolean }) {
  const api = usePlatformApi();
  const reach = api.docLinks;
  useSubscribe((fn) => docLinks.subscribe(fn));
  useEffect(() => void docLinks.load(), []);
  const state = docLinks.get();
  const rows = Array.isArray(state) ? state : null;
  // The machines, reached only once there's a link to ask them about.
  const [, tick] = useState(0);
  const any = !!rows?.length;
  useEffect(() => (any ? reach?.watch(() => tick((n) => n + 1)) : undefined), [any]);
  const [titles, setTitles] = useState<Map<string, string>>(new Map());
  const [removing, setRemoving] = useState<DocLink | null>(null);
  const [link, setLink] = useState<string | null>(null);

  // Each link's title, from the machines that answer (the hub knows none).
  const up = rows ? [...new Set(rows.map((r) => r.daemon))].filter((m) => reach?.here(m)) : [];
  const upKey = up.join(" ");
  useEffect(() => {
    if (!reach || !up.length) return;
    let live = true;
    void Promise.all(up.map((m) => reach.titles(m).catch(() => new Map<string, string>()))).then((all) => {
      if (live) setTitles(new Map(all.flatMap((m) => [...m])));
    });
    return () => void (live = false);
  }, [upKey, rows]);

  // Opened as control's Shared Docs… (`/docs?shared=1`): in view, once it's drawn.
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (focus && any) ref.current?.scrollIntoView({ block: "start" });
  }, [focus, any]);
  if (!rows?.length) return null;

  const here = (r: DocLink) => !!reach?.here(r.daemon);
  const name = (r: DocLink) => reach?.name(r.daemon) ?? "a machine of yours";

  const copy = async (r: DocLink) => {
    try {
      const url = await reach!.url(r.daemon, r.id);
      setLink(null);
      await navigator.clipboard.writeText(url).then(
        () => toast("Link copied"),
        () => setLink(url),
      );
    } catch (e) {
      toast(`Couldn't copy it: ${(e as Error).message}`);
    }
  };
  const open = async (r: DocLink) => {
    try {
      api.goToPane(r.daemon, String(await reach!.open(r.daemon, r.id)), null);
    } catch (e) {
      toast(`Couldn't open it: ${(e as Error).message}`);
    }
  };
  const unshare = async (r: DocLink) => {
    setRemoving(null);
    try {
      if (here(r) && (await reach!.unshare(r.daemon, r.id).catch(() => false))) return void (await docLinks.load(true));
      await docLinks.remove(r.id);
    } catch (e) {
      toast(`Couldn't unshare it: ${(e as Error).message}`);
    }
  };

  return (
    <div ref={ref} data-shared-by-you>
      <ListSection title="Shared by you" n={rows.length} data-docs-shared="">
        <p class="dim" data-shared-docs-why>
          Shared with a link. Arugula cloud keeps them encrypted and can't read them; the link, with its key, is on the machine that shared it.
        </p>
        {rows.map((r) => (
          <ListItem
            key={r.id}
            data-shared-doc={r.id}
            title={<span data-shared-doc-title>{titles.get(r.id) ?? name(r)}</span>}
            detail={
              <>
                {titles.has(r.id) ? `on ${name(r)} · ` : ""}
                {size(r.size)} · shared <Time at={r.made} on /> · updated <Time at={r.updated} on /> · {opened(r.opens)}
              </>
            }
            actions={
              <>
                <button type="button" class="primary small" data-open-shared-doc disabled={!here(r)} title={here(r) ? undefined : OFFLINE} onClick={() => void open(r)}>
                  Open
                </button>
                <button type="button" class="secondary small" data-copy-doc-link disabled={!here(r)} title={here(r) ? undefined : OFFLINE} onClick={() => void copy(r)}>
                  Copy link
                </button>
                <button type="button" class="secondary danger small" data-unshare-doc onClick={() => setRemoving(r)}>
                  Unshare
                </button>
              </>
            }
          />
        ))}
        {link && <CopyText text={link} share data-doc-link-url />}
      </ListSection>
      {removing && (
        <ConfirmRemove title="Unshare this Doc?" label="Unshare" go={() => void unshare(removing)} cancel={() => setRemoving(null)}>
          <p>The link stops working at once. Sharing it again makes a new link; the old one never comes back.</p>
        </ConfirmRemove>
      )}
    </div>
  );
}
