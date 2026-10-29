// arugula.video's page: built into the relay's priv/static, which serves
// it; `pnpm dev` serves it here and sends the socket and the demo token
// to a relay on :4000 (`DEMO=1 mix phx.server` in ../relay).
import { defineConfig } from "vite";

export default defineConfig({
  esbuild: { jsx: "automatic", jsxImportSource: "preact" },
  server: {
    port: 5180,
    fs: { allow: [".."] },
    proxy: {
      "/socket": { target: "ws://127.0.0.1:4000", ws: true },
      "/api": "http://127.0.0.1:4000",
    },
  },
  build: { outDir: "../relay/priv/static", emptyOutDir: true, target: "es2022" },
});
