// The code the site shows (presence/sdk/API.md): every snippet in one
// place, so the landing and the docs never disagree with each other.

export const SDK = "https://arugula.stream/sdk/v0/presence.js";

/** One line each, from arugula.stream itself (presence/sdk/host.sh). */
export const install = {
  React: "npm install https://arugula.stream/npm/presence-react.tgz",
  Node: "npm install https://arugula.stream/npm/presence-server.tgz",
  Python: "pip install https://arugula.stream/py/arugula_presence-0.1.0.tar.gz",
  Go: "go get arugula.stream/presence",
  Elixir: `{:arugula_presence, git: "https://github.com/arugula-salad/arugula", branch: "presence-toolkit", sparse: "presence/sdk/elixir"}`,
  Ruby: "gem install arugula_presence --source https://arugula.stream/gems/",
} as Record<string, string>;

export const browser = {
  HTML: `<script type="module" src="${SDK}"></script>

<arugula-room room="design-team" token-url="/api/presence-token">
  <arugula-camera></arugula-camera>
  <arugula-people></arugula-people>
</arugula-room>`,
  JavaScript: `import { connect } from "${SDK}";

const room = await connect({
  room: "design-team",
  token: () => fetch("/api/presence-token", { method: "POST" })
    .then((r) => r.json()).then((r) => r.token),
});

room.mount(document.querySelector("#people"));
await room.camera.start({ mode: "avatar" });
room.on("wave", (from) => console.log(from, "waved at you"));`,
  React: `import { PresenceRoom, CameraControls, People, Avatar } from "@arugula/presence-react";

export function Team({ me }) {
  return (
    <PresenceRoom room="design-team" token={getToken}>
      <CameraControls />
      <People />
      <Avatar user={me.id} size="head" />
    </PresenceRoom>
  );
}`,
} as const;

export const prototype = `<script type="module" src="${SDK}"></script>

<!-- No server yet? Demo rooms: anyone with the name can join. -->
<arugula-room room="my-prototype" demo name="Ada">
  <arugula-camera></arugula-camera>
  <arugula-people></arugula-people>
</arugula-room>`;

export const server = {
  Node: `import { createRoomToken } from "@arugula/presence-server";

app.post("/api/presence-token", requireUser, (req, res) => {
  res.json({
    token: createRoomToken({
      app: process.env.ARUGULA_APP,
      secret: process.env.ARUGULA_SECRET,
      room: req.body.room,               // check they may join it
      user: { id: req.user.id, name: req.user.name },
    }),
  });
});`,
  Python: `from arugula_presence import create_room_token

@app.post("/api/presence-token")
def presence_token(body: dict, user = Depends(current_user)):
    return {"token": create_room_token(
        app=os.environ["ARUGULA_APP"],
        secret=os.environ["ARUGULA_SECRET"],
        room=body["room"],                 # check they may join it
        user_id=user.id, name=user.name,
    )}`,
  Go: `import "arugula.stream/presence"

func presenceToken(w http.ResponseWriter, r *http.Request) {
	user := currentUser(r) // and check they may join the room
	token, err := presence.CreateRoomToken(presence.TokenOptions{
		App: os.Getenv("ARUGULA_APP"), Secret: os.Getenv("ARUGULA_SECRET"),
		Room: r.FormValue("room"), UserID: user.ID, Name: user.Name,
	})
	if err != nil { http.Error(w, err.Error(), 400); return }
	json.NewEncoder(w).Encode(map[string]string{"token": token})
}`,
  Elixir: `def presence_token(conn, %{"room" => room}) do
  user = conn.assigns.current_user   # and check they may join the room

  token =
    ArugulaPresence.create_room_token(
      app: System.fetch_env!("ARUGULA_APP"),
      secret: System.fetch_env!("ARUGULA_SECRET"),
      room: room, user_id: user.id, name: user.name
    )

  json(conn, %{token: token})
end`,
  Ruby: `post "/api/presence-token" do
  user = current_user # and check they may join the room
  token = ArugulaPresence.create_room_token(
    app: ENV.fetch("ARUGULA_APP"), secret: ENV.fetch("ARUGULA_SECRET"),
    room: params[:room], user_id: user.id, name: user.name
  )
  json token: token
end`,
} as const;

export const jsApi = `const room = await connect({ room, token, relay? });

room.members                  // [{ sub, name, look, mode, away, watching }]
room.on("members", (members) => {})   // also "wave", "answer", "live"
await room.camera.start({ mode: "avatar" | "outline" })
room.camera.stop()
room.setLook({ character, colour })
room.wave(sub)  ·  room.answer(sub, true)
room.mount(el, { size: "bust" })      // everyone, drawn into el
room.avatar(el, sub, { size: "head" })  // one person
room.leave()`;

export const theming = `arugula-room {
  --arugula-accent: #4fd1c5;
  --arugula-bg: #0a0d14;
  --arugula-fg: #e6e9ef;
  --arugula-dim: #7a8496;
  --arugula-radius: 12px;
}`;

export const reactHook = `const { members, wave, camera } = usePresence();`;
