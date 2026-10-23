// Linked products (M229): the products (studio, planter, ...) you let sign
// you in with Arugula, and whose own accounts are linked to yours by your
// numeric GitHub id, never a login. The hub keeps both
// (`/api/linked-products`, through the platform). Unlinking ends both, is
// audited, and the product sees it the next time it asks the hub; it links
// again only when you sign in to it through Arugula again, which asks you
// first. A link is never authorization: it grants nothing here.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";

export type ProductLink = {
  client: string;
  product: string;
  granted?: number;
  user?: string;
  name?: string;
  linked?: number;
};

async function call<T>(path: string, post = false): Promise<T> {
  const r = await platformFetch(path, { method: post ? "POST" : "GET", credentials: "same-origin" });
  const v = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((v as { error?: string }).error ?? `Request failed (${r.status})`);
  return v as T;
}

export function LinkedProducts() {
  const [links, setLinks] = useState<ProductLink[] | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState("");
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
      await call(`/api/linked-products/${encodeURIComponent(l.client)}/unlink`, true);
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  if (links === null) return err ? <p class="control-error">{err}</p> : null;
  return (
    <>
      <h2>Linked products</h2>
      <p class="dim">
        Products you let sign you in with Arugula, and your accounts there linked to this one by your GitHub account's number, never its name.
        Unlinking takes effect the next time that product checks; it links again only if you sign in to it through Arugula again, which asks you
        first.
      </p>
      {links.length === 0 && <p class="dim" data-linked-none>No products linked yet.</p>}
      {links.length > 0 && (
        <ul class="platform-list" data-linked-products>
          {links.map((l) => {
            const since = l.linked ?? l.granted;
            return (
              <li key={l.client} data-linked-product={l.client}>
                <strong>{l.product}</strong>
                {l.user ? (l.name ? <> as {l.name}</> : <> (linked)</>) : <span class="dim"> signs you in, no account linked there</span>}{" "}
                {since ? <span class="dim">since {new Date(since).toLocaleDateString()}</span> : null}{" "}
                <button class="control-linkish" data-unlink={l.client} disabled={busy === l.client} onClick={() => void unlink(l)}>
                  Unlink
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
