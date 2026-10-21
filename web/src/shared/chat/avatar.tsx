// A person's picture in chat: theirs, or their initials on their color.

/** A steady colour per person. */
export function colorOf(who: string): string {
  let h = 0;
  for (const c of who) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 70% 68%)`;
}

/** A message's picture: theirs, or their initials on their color. */
export function MsgAvatar({ who, name, pic }: { who: string; name: string; pic?: string }) {
  const initials = name
    .split(/[\s@]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
  return (
    <span class="msg-avatar" style={{ background: colorOf(who) }} aria-hidden="true">
      {pic ? <img src={pic} alt="" referrerpolicy="no-referrer" /> : initials}
    </span>
  );
}
