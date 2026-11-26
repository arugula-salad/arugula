// Avatar (kit §4.1, 8): a person is their initials on a disc; an agent is
// the ✻ mark on a well. The name is its label, so the initials are never
// the only way to tell who.

import "./kit.css";

/** Up to two initials: the first letters of the first two words. */
export const initials = (name: string) =>
  name
    .split(/[\s@._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");

export function Avatar({ name, agent }: { name: string; agent?: boolean }) {
  return (
    <span class={agent ? "k-avatar is-agent" : "k-avatar"} role="img" aria-label={name} title={name} data-kit="avatar">
      {agent ? "✻" : initials(name)}
    </span>
  );
}
