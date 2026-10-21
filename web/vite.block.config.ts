import { defineConfig } from "vite";

// Control's files for block origins (M50, src/shell/block/), each built by itself
// into dist/block/<mode>.js as one classic script: `--mode boot`, `sw` or
// `shim`. Control serves them at /.arugula/<mode>.js on block origins only
// (crates/control/src/blocks.rs).
export default defineConfig(({ mode }) => {
  if (!["boot", "sw", "shim"].includes(mode)) throw new Error(`vite.block.config.ts: --mode boot, sw or shim, not ${mode}`);
  return {
    publicDir: false,
    build: {
      outDir: "dist/block",
      emptyOutDir: false,
      target: "es2022",
      lib: { entry: `src/shell/block/${mode}.ts`, formats: ["iife"], name: `arugulaBlock_${mode}`, fileName: () => `${mode}.js` },
    },
  };
});
