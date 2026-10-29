import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig, runnerImport, type Plugin } from "vite";
import { releases } from "../scripts/whats-new.mjs";

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

/** The shared Doc's light palette (doc-view.css): Latte as the app draws it
 * (appearance.ts's PALETTES, made legible), written in where doc-view.css
 * says, so the two can't drift (UI audit #180). The page can't import
 * appearance.ts: it would touch the app's cookie. */
function docPalette(): Plugin {
  const MARK = "/* PALETTES: latte */";
  type Appearance = typeof import("./src/shared/appearance.ts");
  let appearance: Promise<Appearance> | null = null;
  return {
    name: "arugula-doc-palette",
    enforce: "pre",
    async transform(code, id) {
      if (!id.split("?")[0].endsWith("/shell/doc-view.css")) return;
      appearance ??= runnerImport<Appearance>(resolve(import.meta.dirname, "src/shared/appearance.ts"), { configFile: false, logLevel: "silent" }).then((r) => r.module);
      const { legible, PALETTES } = await appearance;
      const latte = PALETTES.find((p) => p.id === "latte");
      if (!latte || !code.includes(MARK)) this.error("doc-view.css: no Latte palette or no `/* PALETTES: latte */` to put it in");
      const vars = Object.entries(legible(latte))
        .map(([k, v]) => `--${k}: ${v};`)
        .join("\n    ");
      return { code: code.replace(MARK, vars), map: null };
    },
  };
}

/** This build's version, the workspace's (Cargo.toml): What's new shows
 * the release notes' headlines (scripts/whats-new.mjs) up to it
 * (shared/whats-new.ts). */
function version(): string {
  const toml = readFileSync(resolve(import.meta.dirname, "../Cargo.toml"), "utf8");
  const v = toml.match(/^\[workspace\.package\][^[]*?^version = "([^"]+)"/m)?.[1];
  if (!v) throw new Error("Cargo.toml: no [workspace.package] version");
  return v;
}

export default defineConfig({
  plugins: [docPalette(), modules()],
  define: {
    "import.meta.env.VITE_ARUGULA_VERSION": JSON.stringify(version()),
    "import.meta.env.VITE_WHATS_NEW": JSON.stringify(releases(resolve(import.meta.dirname, ".."))),
  },
  server: {
    port: 5173,
    proxy: { "/ws": { target: "ws://127.0.0.1:7682", ws: true, changeOrigin: true } },
    // The presence packages (People block, chat avatars) are source beside web/.
    fs: { allow: [".", "../presence"] },
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
