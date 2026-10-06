# arugula dev container feature

Puts VS Code (or Cursor) in a dev container in your arugula swarm.

The extension runs inside the container, so it can't see the arugula daemon on the host by itself. This feature mounts the daemon's editors' socket directory (`~/.local/state/arugula/editors` on the machine running the container) at `/run/arugula`, sets `ARUGULA_SOCK`, and asks for the extension. That socket only lets an editor join the swarm; the container can't drive the daemon through it.

```jsonc
// .devcontainer/devcontainer.json
"features": {
  "ghcr.io/jhgaylor/illogical/arugula:0": {}
}
```

Until it's published, copy `src/arugula` into `.devcontainer/arugula` and use `"./arugula": {}`.

The socket is 0600 and belongs to you on the host: the container's user needs your uid (the usual `vscode` user is 1000) or root.
