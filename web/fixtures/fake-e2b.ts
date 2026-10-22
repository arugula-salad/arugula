// A fake E2B (the compute provider hosted workspaces run on, M176): the
// sandbox lifecycle the platform's adapter drives
// (crates/platform/src/compute/e2b.rs), in memory. It makes no box: the
// test stack's hosted-setup claim starts `arugulad --hosted` on a box of
// its own with the env a create was given, which `GET /_testnet/sandboxes`
// answers (with each box's env: a ticket is in it), with the boxes deleted
// and the project each box was made for.

import { createServer, type IncomingMessage } from "node:http";

interface Box {
  sandboxID: string;
  templateID: string;
  state: "running" | "paused";
  metadata: Record<string, string>;
  envVars: Record<string, string>;
  startedAt: string;
  endAt: string;
}

function body(req: IncomingMessage): Promise<string> {
  return new Promise((ok) => {
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => ok(Buffer.concat(chunks).toString()));
  });
}

export function fakeE2b(port: number, host = "127.0.0.1") {
  const boxes = new Map<string, Box>();
  const deleted: string[] = [];
  // Every box's project, deleted ones too, for the test's view.
  const madeFor: Record<string, string> = {};
  let n = 0;
  const detail = (b: Box) => ({
    sandboxID: b.sandboxID,
    templateID: b.templateID,
    state: b.state,
    metadata: b.metadata,
    startedAt: b.startedAt,
    endAt: b.endAt,
    lifecycle: { onTimeout: "pause" },
    cpuCount: 2,
    memoryMB: 2048,
  });
  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? "/", "http://e2b");
    const parts = url.pathname.split("/").filter(Boolean);
    const text = await body(req);
    const reply = (status: number, v?: unknown) => {
      res.writeHead(status, v === undefined ? {} : { "content-type": "application/json" });
      res.end(v === undefined ? undefined : JSON.stringify(v));
    };
    // The test's own view: every box, its env included.
    if (url.pathname === "/_testnet/sandboxes") return reply(200, { boxes: [...boxes.values()], deleted, made: madeFor });
    if (req.headers["x-api-key"] !== "e2b-testnet") return reply(401, { code: 401, message: "bad key" });
    const now = new Date().toISOString();
    const id = parts[0] === "v2" ? parts[2] : parts[1];
    const box = id ? boxes.get(id) : undefined;
    if (req.method === "POST" && url.pathname === "/v2/sandboxes") {
      const b = JSON.parse(text || "{}");
      // As loam: every box is secure, its envd behind an access token.
      if (b.secure !== true) return reply(400, { code: 400, message: "secure: false is refused" });
      const made: Box = {
        sandboxID: `sbx${n++}testnet`,
        templateID: b.templateID,
        state: "running",
        metadata: b.metadata ?? {},
        envVars: b.envVars ?? {},
        startedAt: now,
        endAt: new Date(Date.now() + (b.timeout ?? 300) * 1000).toISOString(),
      };
      boxes.set(made.sandboxID, made);
      madeFor[made.sandboxID] = made.metadata.arugula_project ?? "";
      return reply(201, {
        sandboxID: made.sandboxID,
        templateID: made.templateID,
        clientID: "testnet",
        envdAccessToken: "envd-testnet",
      });
    }
    if (req.method === "GET" && url.pathname === "/v2/sandboxes") {
      // `metadata` is a query string of its own (each pair encoded, then
      // encoded again as one value).
      const want = new URLSearchParams(url.searchParams.get("metadata") ?? "");
      const fits = (b: Box) => [...want].every(([k, v]) => b.metadata[decodeURIComponent(k)] === decodeURIComponent(v));
      return reply(200, [...boxes.values()].filter(fits).map(detail));
    }
    if (parts[0] === "events") return reply(200, []);
    if (!box) return reply(404, { code: 404, message: "no such sandbox" });
    if (req.method === "GET" && parts.length === 2) return reply(200, detail(box));
    if (req.method === "DELETE" && parts.length === 2) {
      boxes.delete(box.sandboxID);
      deleted.push(box.sandboxID);
      return reply(204);
    }
    if (req.method === "POST" && parts.at(-1) === "pause") {
      if (box.state === "paused") return reply(409, { code: 409, message: "paused already" });
      box.state = "paused";
      box.endAt = now;
      return reply(204);
    }
    if (req.method === "POST" && parts.at(-1) === "connect") {
      const woke = box.state === "paused";
      box.state = "running";
      return reply(woke ? 201 : 200, { ...detail(box), envdAccessToken: "envd-testnet" });
    }
    if (req.method === "POST" && parts.at(-1) === "timeout") return reply(204);
    reply(404, { code: 404, message: `no route ${req.method} ${url.pathname}` });
  }).listen(port, host);
  return { server, boxes, deleted, made: madeFor };
}
