// Code on the site: a block with a copy button, and tabs of blocks (one
// language at a time, the choice remembered across the page's tab sets).

import { useState } from "preact/hooks";

export function Code({ code, label }: { code: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () =>
    void navigator.clipboard?.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    });
  return (
    <div class="code" aria-label={label}>
      <button class="copy" onClick={copy} aria-label="Copy the code">
        {copied ? "copied" : "copy"}
      </button>
      <pre>
        <code>{code}</code>
      </pre>
    </div>
  );
}

/** A shell line to copy, as the marketing site shows its install. */
export function Install({ line }: { line: string }) {
  const [copied, setCopied] = useState(false);
  const shell = !line.startsWith("{");
  const copy = () =>
    void navigator.clipboard?.writeText(line).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    });
  return (
    <div class={shell ? "install" : "install dep"}>
      <code>{line}</code>
      <button class="copy" onClick={copy} aria-label="Copy the install line">
        {copied ? "copied" : "copy"}
      </button>
    </div>
  );
}

export function Tabs({ tabs, label, install }: { tabs: Record<string, string>; label: string; install?: Record<string, string> }) {
  const names = Object.keys(tabs);
  const [on, setOn] = useState(names[0]);
  return (
    <div class="tabs">
      <div class="tab-row" role="tablist" aria-label={label}>
        {names.map((n) => (
          <button role="tab" aria-selected={on === n} class={on === n ? "tab on" : "tab"} onClick={() => setOn(n)}>
            {n}
          </button>
        ))}
      </div>
      {install?.[on] && <Install line={install[on]} />}
      <Code code={tabs[on]} label={`${label}: ${on}`} />
    </div>
  );
}
