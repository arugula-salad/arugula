// arugula.stream: presence for your app, upgrading to a call. The front
// page and the docs are for developers; /r/<room> (or ?room=) is a room
// to try it in, served by the relay that carries it.

import { render } from "preact";
import { useState } from "preact/hooks";
import { Landing } from "./landing";
import { Docs } from "./docs";
import { Join, NAME, Room, cleanRoom, keep, kept } from "./room";
import "./style.css";

function roomOf(): string {
  const m = /^\/r\/([^/]+)/.exec(location.pathname);
  return cleanRoom(decodeURIComponent(m?.[1] ?? new URLSearchParams(location.search).get("room") ?? ""));
}

function App() {
  const room = roomOf();
  const [name, setName] = useState(kept(NAME));
  const [joined, setJoined] = useState(false);
  if (location.pathname.startsWith("/docs")) return <Docs />;
  if (!room) return <Landing />;
  if (!joined || !name)
    return (
      <Join
        room={room}
        onName={(n) => {
          keep(NAME, n);
          setName(n);
          setJoined(true);
        }}
      />
    );
  return <Room room={room} name={name} />;
}

render(<App />, document.getElementById("app")!);
