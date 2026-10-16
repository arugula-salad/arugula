/** Transcript messages shared by machine and account-owned agent panes. */
export function AgentMessage({ text, thought = false }: { text: string; thought?: boolean }) {
  return thought ? <details class="agent-thought"><summary>Thinking</summary>{text}</details> : <div class="agent-msg">{text}</div>;
}
