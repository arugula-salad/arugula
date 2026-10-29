// meet.arugula.stream: built into the relay's priv/static/meet, which the
// relay serves on a meet. host and under /meet on any host. Its files are
// under /meet/ either way (base); the app reads its own base from the
// page's path. `pnpm dev` serves it at :5181/meet/ and sends the API, the
// sign-in pages, the socket and the trackers' models to a relay on :4000.
import { defineConfig } from "vite";

const relay = "http://127.0.0.1:4000";

export default defineConfig({
  base: "/meet/",
  esbuild: { jsx: "automatic", jsxImportSource: "preact" },
  server: {
    port: 5181,
    fs: { allow: [".."] },
    proxy: {
      "/socket": { target: "ws://127.0.0.1:4000", ws: true },
      "/api": relay,
      "/auth": relay,
      "/mediapipe": relay,
      "/depth": relay,
    },
  },
  build: { outDir: "../relay/priv/static/meet", emptyOutDir: true, target: "es2022" },
});
