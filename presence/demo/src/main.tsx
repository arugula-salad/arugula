// arugula.stream: presence for your app, upgrading to a call. The front
// page is the hosted pitch, /developers the setup, /docs the reference;
// /r/<room> (or ?room=) is a room to try it in, served by the relay that
// carries it.

import { render } from "preact";
import { useState } from "preact/hooks";
import { Developers } from "./landing";
import { Docs } from "./docs";
import { Hosted } from "./hosted";
import { Privacy, Terms } from "./legal";
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
  // The hosted pitch is the front page now; old links still land on it.
  if (location.pathname.startsWith("/hosted")) {
    history.replaceState(null, "", "/" + location.hash);
    return <Hosted />;
  }
  if (location.pathname.startsWith("/developers")) return <Developers />;
  if (location.pathname.startsWith("/privacy")) return <Privacy />;
  if (location.pathname.startsWith("/terms")) return <Terms />;
  if (!room) return <Hosted />;
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
