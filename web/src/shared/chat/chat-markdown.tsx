// A chat message's Markdown (#109, #105): the shared renderer with what only
// chat loads, the emoji table (when a message has a shortcode) and code
// blocks with colours and a copy button. Kept out of ui/markdown, which a
// Doc's share page bundles.

import { useMemo } from "preact/hooks";
import { CodeBlock } from "../ui/codeblock";
import { Markdown, type ChatKit } from "../ui/markdown";
import type { IsMe } from "../ui/markup";
import { SHORTCODE, onlyEmoji, useEmoji, withEmoji } from "./emoji";

export function ChatMarkdown({ text, isMe, landed }: { text: string; isMe?: IsMe; landed?: (token: string) => boolean }) {
  // The shortcode table comes only for a text that has one.
  const table = useEmoji(SHORTCODE.test(text));
  const kit = useMemo<ChatKit>(
    () => ({ text: table ? (s) => withEmoji(s, table) : undefined, jumbo: onlyEmoji(text, table), Code: CodeBlock }),
    [text, table],
  );
  return <Markdown text={text} refs={null} chat={kit} isMe={isMe} landed={landed} />;
}
