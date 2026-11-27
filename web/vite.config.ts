import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig, runnerImport, type Plugin } from "vite";
import { releases } from "../scripts/whats-new.mjs";

// `just dev` runs a dev daemon on 7682 that accepts this server's origin.
// Four pages: the app, the read-only viewer behind share links, the
// viewer of a shared Doc (doc.html, #1066) and a project Doc on its own for
// the native app's web view (embed.html, #1258). This is the shell, which the
// daemon embeds: its module ids go to dist.modules.json for the bundle
// check (`check-imports.mjs --bundle … --forbid platform`, M155), so none of
// them may be the platform's or the app's.

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

/** The shared Doc's house pair (doc-view.css): Arugula at its own
 * sliders, dark and light (kit/look.ts's paint), with a Doc's tokens
 * (house), written in where doc-view.css says, so the two can't drift (UI
 * audit #180, #729, #1511). The page can't import appearance.ts: it would
 * touch the app's cookie. */
function docPalette(): Plugin {
  const MARKS = { "/* HOUSE: dark */": true, "/* HOUSE: light */": false } as const;
  type Look = typeof import("./src/shared/kit/look.ts");
  let look: Promise<Look> | null = null;
  return {
    name: "arugula-doc-palette",
    enforce: "pre",
    async transform(code, id) {
      // The Doc's embed page (#1258) takes doc-view.css by @import, which
      // this doesn't see, so its own stylesheet has the markers too.
      if (!/\/shell\/doc-(view|embed)\.css$/.test(id.split("?")[0])) return;
      look ??= runnerImport<Look>(resolve(import.meta.dirname, "src/shared/kit/look.ts"), { configFile: false, logLevel: "silent" }).then((r) => r.module);
      const { house, paint, sliders } = await look;
      for (const [mark, dark] of Object.entries(MARKS)) {
        if (!code.includes(mark)) this.error(`${id}: no \`${mark}\` to put the house pair in`);
        const vars = [...Object.entries(paint(dark, sliders(dark))).map(([k, v]) => `--${k}: ${v};`), ...Object.entries(house(dark)).map(([k, v]) => `--doc-${k}: ${v};`)].join("\n    ");
        code = code.replace(mark, vars);
      }
      return { code, map: null };
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
    // `/auth`: open http://localhost:5173/auth?token=… (the dev daemon's
    // token) and the sign-in page gives this origin its page key.
    proxy: {
      "/ws": { target: "ws://127.0.0.1:7682", ws: true, changeOrigin: true },
      "/auth": { target: "http://127.0.0.1:7682", changeOrigin: true },
      "/proto/": { target: "http://127.0.0.1:7682", changeOrigin: true },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: { input: { main: "index.html", share: "share.html", doc: "doc.html", embed: "embed.html" } },
    // Follow mode's CodeMirror (M28) is a chunk of its own, about 510 kB
    // (180 kB gzipped), loaded only when someone follows an editor.
    chunkSizeWarningLimit: 560,
  },
});
