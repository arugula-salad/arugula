// Whole numbers JavaScript can't hold exactly (#889). A pane's epoch is
// nanoseconds, above 2^53: `JSON.parse` rounds it (…095103 comes back as
// …095000). The wire keeps it a JSON number, which every daemon reads
// exactly, so older ones keep working: the browser reads its digits from
// the response's text and writes them back into a request's.

/** A request body already written as JSON, sent as it is. */
export class JsonText {
  readonly text: string;
  constructor(text: string) {
    this.text = text;
  }
}

/** The digits of `key`'s whole-number value in a JSON object's text (the
 * first, at any depth: callers ask of flat objects). Inside a string a key's
 * quotes are escaped, so a title can't pass for one. */
export function exactField(text: string, key: string): string | null {
  return new RegExp(`(?<!\\\\)"${key}"\\s*:\\s*(\\d+)\\b`).exec(text)?.[1] ?? null;
}

/** `body` as JSON, with `key` set to the whole number `digits` exactly. */
export function withExact(body: object, key: string, digits: string): JsonText | object {
  if (!/^\d+$/.test(digits)) return body;
  const rest = Object.fromEntries(Object.entries(body).filter(([k]) => k !== key));
  const tail = JSON.stringify(rest).slice(1);
  return new JsonText(`{${JSON.stringify(key)}:${digits}${tail === "}" ? "}" : `,${tail}`}`);
}
