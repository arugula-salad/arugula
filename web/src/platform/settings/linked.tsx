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
                {l.projects !== undefined && (
                  <label class="platform-mirror">
                    <input
                      type="checkbox"
                      data-linked-projects={l.client}
                      checked={l.projects}
                      disabled={busy === l.client}
                      onChange={() => void sees(l)}
                    />{" "}
                    Let {l.product} see my projects and my role in each, and keep its apps in the ones I edit
                  </label>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
