// arugula.stream's dashboard: built into the relay's priv/static/dashboard,
// which the relay serves under /dashboard. `pnpm dev` serves it at
// :5182/dashboard/ and sends the API and the sign-in pages to a relay on
// :4000 (RELAY moves it).
import { defineConfig } from "vite";

const relay = process.env.RELAY ?? "http://127.0.0.1:4000";

export default defineConfig({
  base: "/dashboard/",
  esbuild: { jsx: "automatic", jsxImportSource: "preact" },
  server: {
    port: 5182,
    fs: { allow: [".."] },
    proxy: { "/api": relay, "/auth": relay },
  },
  build: { outDir: "../relay/priv/static/dashboard", emptyOutDir: true, target: "es2022" },
});
