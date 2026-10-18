# Arugula dev container feature

Puts VS Code (or Cursor) in a dev container in your Arugula swarm.

The extension runs inside the container, so it can't see the Arugula daemon on the host by itself. This feature mounts the daemon's editors' socket directory (`~/.local/state/arugula/editors` on the machine running the container) at `/run/arugula`, sets `ARUGULA_SOCK`, and asks for the extension. That socket only lets an editor join the swarm; the container can't drive the daemon through it.

```jsonc
// .devcontainer/devcontainer.json
"features": {
  "ghcr.io/arugula-salad/arugula/arugula:0": {}
}
```

Until it's published, copy `src/arugula` into `.devcontainer/arugula` and use `"./arugula": {}`. It was never published under its old name (`illogical`), so there's no old feature id to keep; a copy of the old `src/illogical` keeps working on a machine whose daemon started out as illogical, since its socket is still found through the old path.

On a machine whose daemon started out as illogical, the daemon moves the state directory to `~/.local/state/arugula` and leaves `~/.local/state/illogical` as a link to it, so the mount's source is there either way (on Linux and macOS). The extension it asks for, `arugula.arugula-editor`, comes from the Visual Studio Marketplace (or Open VSX, in editors that use it).

The socket is 0600 and belongs to you on the host: the container's user needs your uid (the usual `vscode` user is 1000) or root.
