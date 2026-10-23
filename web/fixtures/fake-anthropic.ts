// A fake Anthropic (the inference broker's upstream, M231): the Messages API
// as Claude Code and the SDKs call it, in memory. A message answers 100 input
// tokens and `max_tokens` output tokens, streamed as server-sent events when
// it asks to be; a key with "revoked" in it is refused (401). It records the
// credential each request carried and every header name, which
// `GET /_testnet/seen` answers, so the testnet's broker claim checks which
// key paid and that nothing of the caller's went upstream.

import { createServer, type IncomingMessage } from "node:http";

function body(req: IncomingMessage): Promise<string> {
  return new Promise((ok) => {
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => ok(Buffer.concat(chunks).toString()));
  });
}

/** What the fake streams for a message of `model` with `out` output tokens. */
export function events(model: string, out: number): string {
  return [
    `event: message_start\ndata: {"type":"message_start","message":{"id":"msg_1","model":"${model}","usage":{"input_tokens":100,"output_tokens":1}}}\n\n`,
    `event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}\n\n`,
    `event: message_delta\ndata: {"type":"message_delta","usage":{"output_tokens":${out}}}\n\n`,
    `event: message_stop\ndata: {"type":"message_stop"}\n\n`,
  ].join("");
}

interface Seen {
  path: string;
  key: string;
  headers: string[];
  /** Every header's value, joined: what a leak would show up in. */
  values: string;
}

export function fakeAnthropic(port: number, host = "127.0.0.1") {
  const seen: Seen[] = [];
  return createServer(async (req, res) => {
    const u = new URL(req.url!, `http://${host}:${port}`);
    const text = await body(req);
    if (u.pathname === "/_testnet/seen") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify(seen));
    }
    const h = req.headers;
    const auth = String(h.authorization ?? "");
    const key = String(h["x-api-key"] ?? (auth.startsWith("Bearer ") ? auth.slice(7) : ""));
    seen.push({ path: u.pathname, key, headers: Object.keys(h).sort(), values: Object.values(h).join("\n") });
    const json = (status: number, v: unknown) => {
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(v));
    };
    if (key.includes("revoked")) return json(401, { type: "error", error: { type: "authentication_error", message: "invalid x-api-key" } });
    if (req.method === "GET" && u.pathname.startsWith("/v1/models")) return json(200, { data: [{ id: "claude-opus-5" }] });
    if (req.method === "POST" && u.pathname === "/v1/messages/count_tokens") return json(200, { input_tokens: 7 });
    if (req.method !== "POST" || u.pathname !== "/v1/messages") return json(404, { type: "error", error: { type: "not_found_error" } });
    let m: { model?: string; max_tokens?: number; stream?: boolean } = {};
    try {
      m = JSON.parse(text);
    } catch {}
    const model = m.model ?? "claude-opus-5";
    const out = m.max_tokens ?? 10;
    if (m.stream) {
      res.writeHead(200, { "content-type": "text/event-stream" });
      return res.end(events(model, out));
    }
    json(200, { type: "message", id: "msg_2", model, content: [{ type: "text", text: "hi" }], usage: { input_tokens: 100, output_tokens: out } });
  }).listen(port, host);
}
