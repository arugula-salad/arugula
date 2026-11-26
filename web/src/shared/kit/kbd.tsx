// Kbd (kit §4.1, 7): a key, as a small raised chip. `keys` is one key or a
// sequence ("g o" is two chips).

import "./kit.css";

export function Kbd({ keys }: { keys: string }) {
  return (
    <>
      {keys.split(" ").map((k, i) => (
        <kbd key={i} class="k-kbd" data-kit="kbd">
          {k}
        </kbd>
      ))}
    </>
  );
}
