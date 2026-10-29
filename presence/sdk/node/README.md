# @arugula/presence-server (Node)

Sign arugula.stream room tokens on your server. `node:crypto`, no
dependencies, Node 18+. The token says which room a user may join and
what to call them; your page's `<arugula-room token-url>` (or `connect`)
fetches it from your endpoint.

Install (from this repo, until it's on npm):

    npm install ./presence/sdk/node

    import { createRoomToken } from "@arugula/presence-server";
    const token = createRoomToken({ app: "acme", secret: process.env.ARUGULA_SECRET, room: "design-team", user: { id: user.id, name: user.name } });

Options: `app`, `secret`, `room`, `user: {id, name}` (required); `look:
{character, colour}`, `ttl` (seconds, default 3600, at most 86400), `now`
(tests). Throws on bad input (see `index.d.ts`).

Express endpoint:

```js
import express from "express";
import { createRoomToken } from "@arugula/presence-server";

const app = express().use(express.json());
app.post("/api/presence-token", requireLogin, (req, res) => {
  // Your rule for who may be in which room goes here.
  if (!req.user.teams.includes(req.body.room)) return res.sendStatus(403);
  res.json({ token: createRoomToken({ app: "acme", secret: process.env.ARUGULA_SECRET, room: req.body.room, user: { id: req.user.id, name: req.user.name } }) });
});
```

Tests: `npm test` (every token in `../vectors.json`, byte for byte).
