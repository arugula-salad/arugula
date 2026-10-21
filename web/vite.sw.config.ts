import { defineConfig } from "vite";

// The service worker (src/shell/sw.ts), built by itself after the app into
// dist/sw.js: one classic script, with the end-to-end channel code it
// answers notifications over (M29) inlined.
export default defineConfig({
  publicDir: false,
  build: {
    outDir: "dist",
    emptyOutDir: false,
    target: "es2022",
    lib: { entry: "src/shell/sw.ts", formats: ["iife"], name: "arugulaSw", fileName: () => "sw.js" },
  },
});
