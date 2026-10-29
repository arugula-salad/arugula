// `npm install https://arugula.stream/npm/presence-react.tgz`: this package
// bundled into one ES module (plus the trackers' lazy chunks), React left
// to the app (a peer). presence/sdk/host.sh packs pack/ with dist/ in it.
// Not library mode, which would inline ONNX Runtime's Wasm (see
// ../elements/vite.config.ts); its unused copy is dropped here too.
import { defineConfig, type Plugin } from "vite";

const dropWasm: Plugin = {
  name: "drop-unused-wasm",
  generateBundle(_, bundle) {
    for (const name of Object.keys(bundle)) if (name.endsWith(".wasm")) delete bundle[name];
  },
};

export default defineConfig({
  plugins: [dropWasm],
  esbuild: { jsx: "automatic" },
  // Chunks load relative to this module, wherever it's served: no
  // modulepreload links (they'd point at the host page's root).
  base: "./",
  build: {
    modulePreload: false,
    outDir: "pack/dist",
    emptyOutDir: true,
    target: "es2022",
    minify: false,
    rollupOptions: {
      input: "src/index.tsx",
      preserveEntrySignatures: "strict",
      external: ["react", "react-dom", "react/jsx-runtime"],
      output: { entryFileNames: "index.js", chunkFileNames: "[name]-[hash].js", assetFileNames: "[name]-[hash][extname]" },
    },
  },
});
