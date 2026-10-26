// A read-only link's page (M19, `#link=…`) says what it is (UI audit
// #149): the viewer watches and can't type, and the link ends, at a time
// the link carries when it was made since (else it says only read-only).

import { useEffect, useState } from "preact/hooks";
import { Time } from "../../shared/ui/time";

/** Undefined: not a link's page. Null: one that doesn't say when it ends. */
let link: { ends: number | null } | undefined;

export function setReadOnlyLink(ends: number | null) {
  link = { ends: ends && Number.isFinite(ends) ? ends : null };
}

export function LinkNote() {
  const ends = link?.ends ?? null;
  const [over, setOver] = useState(() => ends !== null && ends <= Date.now());
  useEffect(() => {
    if (ends === null || over) return;
    const t = setTimeout(() => setOver(true), ends - Date.now());
    return () => clearTimeout(t);
  }, [ends, over]);
  if (!link) return null;
  return (
    <div class="ws-stale maintenance-note" role="status" data-read-only-link={ends ?? ""}>
      Read-only link
      {ends !== null ? (
        <>
          {" "}
          · {over ? "ended" : "ends"} <Time at={ends} />
        </>
      ) : (
        ""
      )}
    </div>
  );
}
