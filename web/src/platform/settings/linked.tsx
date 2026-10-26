// Linked products (M229): the products (studio, planter, ...) you let sign
// you in with Arugula, and whose own accounts are linked to yours by your
// numeric GitHub id, never a login. The hub keeps both
// (`/api/linked-products`, through the platform). Unlinking ends both, is
// audited, and the product sees it the next time it asks the hub; it links
// again only when you sign in to it through Arugula again, which asks you
// first. A link is never authorization: it grants nothing here. A product
// that's also a client of the platform (studio, M236) gets a switch of its
// own beside it: whether it may see your projects and your role in each,
// and keep its apps in the ones you edit. Off until you turn it on;
// unlinking ends it.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { toneClass } from "../swarm";
import { Section } from "./section";

export type ProductLink = {
  client: string;
  product: string;
  granted?: number;
  user?: string;
  name?: string;
  linked?: number;
  /** Whether it may see your projects (M236); missing when it isn't a client of the platform. */
  projects?: boolean;
};

async function call<T>(path: string, method = "GET"): Promise<T> {
  const r = await platformFetch(path, { method, credentials: "same-origin" });
  const v = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((v as { error?: string }).error ?? `Request failed (${r.status})`);
  return v as T;
}

export function LinkedProducts() {
  const [links, setLinks] = useState<ProductLink[] | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState("");
  // Unlink asks first (UI audit #158).
  const [unlinking, setUnlinking] = useState<ProductLink | null>(null);
  const load = () =>
    call<{ links: ProductLink[] }>("/api/linked-products").then(
      (v) => setLinks(v.links),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), []);
  const unlink = async (l: ProductLink) => {
    setErr("");
    setBusy(l.client);
    try {
      await call(`/api/linked-products/${encodeURIComponent(l.client)}/unlink`, "POST");
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  const sees = async (l: ProductLink) => {
    setErr("");
    setBusy(l.client);
    try {
      await call(`/api/linked-products/${encodeURIComponent(l.client)}/projects`, l.projects ? "DELETE" : "PUT");
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  if (links === null) return err ? <p class="control-error">{err}</p> : null;
  return (
    <Section
      id="linked"
      title="Linked products"
      lede="Products you let sign you in with Arugula. Linking alone grants them nothing here."
      more={
        <p>
          Your accounts there are linked to this one by your GitHub account's number, never its name. Unlinking takes effect the next time
          that product checks; it links again only if you sign in to it through Arugula again, which asks you first.
        </p>
      }
    >
      {links.length === 0 && (
        <p class="dim" data-linked-none>
          No products linked yet.
        </p>
      )}
      {links.length > 0 && (
        <ul class="platform-list sw-cards settings-cards" data-linked-products>
          {links.map((l) => {
            const since = l.linked ?? l.granted;
            // The bar: whether an account of yours there is linked, or it only signs you in.
            return (
              <li key={l.client} class={`sw-card ${toneClass(l.user ? "accent" : "muted")}`} data-linked-product={l.client}>
                <div class="sw-head">
                  <b class="sw-name">{l.product}</b>
                  <button class="secondary danger small settings-card-act" data-unlink={l.client} disabled={busy === l.client} onClick={() => setUnlinking(l)}>
                    Unlink
                  </button>
                </div>
                <span class="dim">
                  {l.user ? (l.name ? <>Linked as {l.name}</> : "Linked") : "Signs you in, no account linked there"}
                  {since ? <> · since {new Date(since).toLocaleDateString()}</> : null}
                </span>
                {l.projects !== undefined && (
                  <label class="settings-switch">
                    <input
                      type="checkbox"
                      role="switch"
                      class="sw-switch"
                      data-linked-projects={l.client}
                      checked={l.projects}
                      disabled={busy === l.client}
                      onChange={() => void sees(l)}
                    />
                    <span>Let {l.product} see my projects and my role in each, and keep its apps in the ones I edit</span>
                  </label>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {err ? <p class="control-error">{err}</p> : null}
      {unlinking && (
        <ConfirmRemove
          title={`Unlink ${unlinking.product}?`}
          label="Unlink"
          cancel={() => setUnlinking(null)}
          go={() => {
            const l = unlinking;
            setUnlinking(null);
            void unlink(l);
          }}
        >
          <p>It stops signing you in with Arugula, and your account there is no longer linked to this one. Signing in to it through Arugula again links it again.</p>
        </ConfirmRemove>
      )}
    </Section>
  );
}
