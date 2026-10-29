// The script-tag SDK: one ES module at the relay's /sdk/v0/presence.js,
// the trackers' and models' code in lazy chunks beside it. Builds into
// the relay's priv/static/sdk/v0 only (the demo builds the rest of
// priv/static, first). Not library mode: that inlines every asset (ONNX
// Runtime's Wasm, as base64, into the depth chunk).
import { defineConfig, type Plugin } from "vite";

/** ONNX Runtime's Wasm, which transformers.js would load from beside its
 * chunk: presence-core points it at the relay's /depth/ instead, so the
 * copy here is dead weight (26 MB). */
const dropWasm: Plugin = {
  name: "drop-unused-wasm",
  generateBundle(_, bundle) {
    for (const name of Object.keys(bundle)) if (name.endsWith(".wasm")) delete bundle[name];
  },
};

export default defineConfig({
  plugins: [dropWasm],
  // Chunks load relative to this module, wherever it's served: no
  // modulepreload links (they'd point at the host page's root).
  base: "./",
  build: {
    modulePreload: false,
    outDir: "../relay/priv/static/sdk/v0",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: {
      input: "src/index.ts",
      preserveEntrySignatures: "strict",
      output: { entryFileNames: "presence.js", chunkFileNames: "[name]-[hash].js", assetFileNames: "[name]-[hash][extname]" },
    },
  },
});
