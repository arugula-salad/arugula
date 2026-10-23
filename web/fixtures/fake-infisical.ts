// A fake Infisical for the platform's projection (M235), answering only
// what crates/platform/fixtures/infisical/answers.json captured (its README
// says from where): universal-auth login, folders listed and made, a secret
// created or, once it exists, overwritten. It never answers a value or a
// delete. `GET /_testnet/seen` says what it holds and every request it got,
// for the test stack's `redone` claim.
//
//   fakeInfisical(port, host, answersPath, clientSecret)

import { readFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server } from "node:http";

function body(req: IncomingMessage): Promise<string> {
  return new Promise((ok) => {
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => ok(Buffer.concat(chunks).toString()));
  });
}

export function fakeInfisical(port: number, host: string, answersPath: string, clientSecret: string, folders: string[] = ["beds"]): Server {
  const answers = JSON.parse(readFileSync(answersPath, "utf8"));
  const held: Record<string, Record<string, string>> = {};
  const made = new Set(folders);
  const seen: { method: string; path: string; auth: string }[] = [];
  const token = answers.login.body.accessToken as string;
  return createServer(async (req, res) => {
    const u = new URL(req.url!, `http://${host}:${port}`);
    const raw = await body(req);
    const b = raw ? JSON.parse(raw) : {};
    const reply = (name: string, fill: Record<string, string> = {}) => {
      let text = JSON.stringify(answers[name].body);
      for (const [k, v] of Object.entries(fill)) text = text.replaceAll(`{${k}}`, JSON.stringify(v).slice(1, -1));
      res.writeHead(answers[name].status, { "content-type": "application/json" }).end(text);
    };
    if (u.pathname === "/_testnet/seen") {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ folders: [...made], secrets: held, seen }));
      return;
    }
    seen.push({ method: req.method!, path: u.pathname + u.search, auth: String(req.headers.authorization ?? "") });
    if (u.pathname === "/api/v1/auth/universal-auth/login") return reply(b.clientSecret === clientSecret ? "login" : "login_refused");
    if (req.headers.authorization !== `Bearer ${token}`) return reply("login_refused");
    if (req.method === "GET" && u.pathname === "/api/v1/folders") {
      const tpl = answers.folders.body.folders[0];
      const list = [...made].map((name) => ({ ...tpl, name, path: `/${name}` }));
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ folders: list }));
      return;
    }
    if (req.method === "POST" && u.pathname === "/api/v1/folders") {
      if (made.has(b.name)) return reply("folder_exists");
      made.add(b.name);
      return reply("folder_created", { name: b.name });
    }
    const m = /^\/api\/v3\/secrets\/raw\/([A-Za-z_][A-Za-z0-9_]*)$/.exec(u.pathname);
    if (m && (req.method === "POST" || req.method === "PATCH")) {
      const folder = String(b.secretPath ?? "/").replace(/^\//, "");
      const there = held[folder]?.[m[1]] !== undefined;
      if (req.method === "POST" && there) return reply("secret_exists");
      (held[folder] ??= {})[m[1]] = String(b.secretValue);
      return reply("secret_written", { key: m[1], value: String(b.secretValue), project: b.workspaceId, environment: b.environment });
    }
    reply("forbidden");
  }).listen(port, host);
}
