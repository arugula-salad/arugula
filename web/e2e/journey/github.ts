// A fake GitHub for the journeys (#551), plain enough to load in a Node
// script as well as in Playwright (testnet/macos/journey-j1.ts).

import { createServer, type IncomingMessage, type Server } from "node:http";
import { githubId } from "../../fixtures/fakes.ts";

function body(req: IncomingMessage): Promise<string> {
  return new Promise((ok) => {
    let b = "";
    req.on("data", (d) => (b += d));
    req.on("end", () => ok(b));
  });
}

/** GitHub as a first-time user meets it: signed in to GitHub already
 * (as the `gh_user` cookie says, else `fallback`: a real Safari the test
 * can't set cookies in), asked once to authorize the OAuth app. */
export function fakeGithub(fallback = "nobody"): Server {
  return createServer(async (req, res) => {
    const u = new URL(req.url!, "http://github");
    const cookies = req.headers.cookie ?? "";
    const who = /(?:^|;\s*)gh_user=(\w+)/.exec(cookies)?.[1] ?? fallback;
    if (u.pathname === "/login/oauth/authorize") {
      const authorized = new RegExp(`(?:^|;\\s*)gh_ok_${who}=1`).test(cookies);
      if (!authorized && u.searchParams.get("authorize") !== "1") {
        u.searchParams.set("authorize", "1");
        res
          .writeHead(200, { "content-type": "text/html" })
          .end(
            `<!doctype html><title>Authorize application</title><main style="font:16px system-ui;max-width:28rem;margin:4rem auto">` +
              `<h1>Authorize Arugula</h1><p>Arugula by arugula-salad wants to access your <b>${who}</b> account.</p>` +
              `<p>Public data only: your login and avatar.</p>` +
              `<a role="button" href="${u.pathname}${u.search}" style="display:inline-block;padding:8px 16px;background:#1f883d;color:#fff;border-radius:6px;text-decoration:none">Authorize arugula-salad</a></main>`,
          );
        return;
      }
      const back = new URL(u.searchParams.get("redirect_uri")!);
      back.searchParams.set("code", `c0de.${who}`);
      back.searchParams.set("state", u.searchParams.get("state")!);
      res
        .writeHead(302, {
          location: back.href,
          "set-cookie": `gh_ok_${who}=1; Path=/`,
        })
        .end();
    } else if (u.pathname === "/login/oauth/access_token") {
      const code = new URLSearchParams(await body(req)).get("code") ?? "";
      res.writeHead(200, { "content-type": "application/json" }).end(
        JSON.stringify({
          access_token: `gho_${code.replace(/^c0de\./, "")}`,
        }),
      );
    } else if (u.pathname === "/user") {
      const login = String(req.headers.authorization ?? "").replace(/^(Bearer|token) gho_/, "");
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ id: githubId(login), login }));
    } else res.writeHead(404).end();
  });
}
