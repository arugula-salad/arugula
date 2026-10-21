import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig, type Plugin } from "vite";

// `just dev` runs a dev daemon on 7682 that accepts this server's origin.
// Three pages: the app, the read-only viewer behind share links, and the
// viewer of a shared Doc (doc.html, #1066). This is the shell, which the
// daemon embeds: its module ids go to dist.modules.json for the bundle
// check (`check-imports.mjs --bundle … --forbid platform`, M155), so none of
// them may be the platform page's.

/** The build's module ids, for the bundle check. */
function modules(): Plugin {
  return {
    name: "arugula-shell-modules",
    apply: "build",
    generateBundle() {
      writeFileSync(resolve(import.meta.dirname, "dist.modules.json"), JSON.stringify([...this.getModuleIds()].sort(), null, 1));
    },
  };
}

export default defineConfig({
  plugins: [modules()],
  server: {
    port: 5173,
    proxy: { "/ws": { target: "ws://127.0.0.1:7682", ws: true, changeOrigin: true } },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: { input: { main: "index.html", share: "share.html", doc: "doc.html" } },
    // Follow mode's CodeMirror (M28) is a chunk of its own, about 510 kB
    // (180 kB gzipped), loaded only when someone follows an editor.
    chunkSizeWarningLimit: 560,
  },
});
