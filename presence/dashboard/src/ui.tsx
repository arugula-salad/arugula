// The dashboard's small parts: copying, code blocks and their tabs (as
// the docs show them: ../../demo/src/code.tsx), a two-step button for
// what can't be undone, and a section card.

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";

/** Copies `text`, then says so for a moment. */
export function useCopy(): [string, (text: string, what?: string) => void] {
  const [copied, setCopied] = useState("");
  const copy = (text: string, what = text) =>
    void navigator.clipboard?.writeText(text).then(() => {
      setCopied(what);
      setTimeout(() => setCopied((c) => (c === what ? "" : c)), 1400);
    });
  return [copied, copy];
}

/** A button that copies `text`: "Copy" (or `label`), then "Copied". */
export function CopyButton({ text, label = "Copy", name }: { text: string; label?: string; name?: string }) {
  const [copied, copy] = useCopy();
  return (
    <button class="btn small" onClick={() => copy(text)} aria-label={copied ? "Copied" : name}>
      {copied ? "Copied" : label}
    </button>
  );
}

export function Code({ code, label }: { code: string; label?: string }) {
  const [copied, copy] = useCopy();
  return (
    <div class="code" aria-label={label}>
      <button class="copy" onClick={() => copy(code)} aria-label="Copy the code">
        {copied ? "copied" : "copy"}
      </button>
      <pre>
        <code>{code}</code>
      </pre>
    </div>
  );
}

export function Tabs({ tabs, label }: { tabs: Record<string, string>; label: string }) {
  const names = Object.keys(tabs);
  const [on, setOn] = useState(names[0]!);
  const shown = tabs[on] === undefined ? names[0]! : on;
  return (
    <div class="tabs">
      <div class="tab-row" role="tablist" aria-label={label}>
        {names.map((n) => (
          <button role="tab" aria-selected={shown === n} class={shown === n ? "tab on" : "tab"} onClick={() => setOn(n)}>
            {n}
          </button>
        ))}
      </div>
      <Code code={tabs[shown]!} label={`${label}: ${shown}`} />
    </div>
  );
}

/** A button that asks first: `label`, then the question with yes and no. */
export function Confirm({ label, question, yes, onYes, name }: { label: string; question: string; yes: string; onYes: () => Promise<unknown> | void; name?: string }) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!asking)
    return (
      <button class="btn small" onClick={() => setAsking(true)} aria-label={name}>
        {label}
      </button>
    );
  return (
    <span class="confirm" role="group" aria-label={question}>
      <span>{question}</span>
      <button
        class="btn small danger"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          try {
            await onYes();
          } finally {
            setBusy(false);
            setAsking(false);
          }
        }}
      >
        {yes}
      </button>
      <button class="btn small" onClick={() => setAsking(false)}>
        Cancel
      </button>
    </span>
  );
}

/** One of an app's sections: a card with a kind colour's rail. */
export function Section({ id, title, kind, sub, children }: { id: string; title: string; kind: string; sub?: ComponentChildren; children: ComponentChildren }) {
  return (
    <section id={id} class="card panel" style={{ "--c": `var(${kind})` }} aria-labelledby={`${id}-h`}>
      <h2 id={`${id}-h`}>{title}</h2>
      {sub && <p class="sub">{sub}</p>}
      {children}
    </section>
  );
}

/** A labelled input with its error under it. */
export function Field({ label, hint, error, children }: { label: string; hint?: ComponentChildren; error?: string; children: ComponentChildren }) {
  return (
    <label class={error ? "field bad" : "field"}>
      <span class="label">{label}</span>
      {children}
      {error && (
        <span class="error" role="alert">
          {error}
        </span>
      )}
      {hint && <span class="hint">{hint}</span>}
    </label>
  );
}

export const value = (e: Event) => (e.target as HTMLInputElement).value;
