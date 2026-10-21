import { Markdown } from "./ui/markdown";

/** Transcript messages shared by machine and account-owned agent panes. */
export function AgentMessage({ text, thought = false }: { text: string; thought?: boolean }) {
  const body = <Markdown text={text} refs={null} />;
  return thought ? <details class="agent-thought"><summary>Thinking</summary>{body}</details> : <div class="agent-msg">{body}</div>;
}
