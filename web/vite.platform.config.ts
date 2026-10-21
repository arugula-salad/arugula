import { copyFileSync, renameSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig, type Plugin } from "vite";

// The platform page (M150): its own build of app.html into dist-platform/,
// which the platform binary embeds and serves on app.arugula.io through the
// hub (M151). Its module ids go to dist-platform.modules.json for the
// bundle check (`check-imports.mjs --bundle … --forbid keys`, M148): none of
// them may be the shell's.
//
// `just dev-platform` serves it on 127.0.0.1:7691 next to `just dev-control`
// (7690), with control's paths sent there: the browser's session cookie is
// the same host's, so signing in on control's page signs this page in.
const control = process.env.DEV_CONTROL_ORIGIN ?? `http://127.0.0.1:${process.env.DEV_CONTROL_PORT ?? 7690}`;

/** The build's module ids, for the bundle check; app.html as index.html;
 * and the icons, without the shell's other public files (its manifest, the
 * key probe), which aren't the platform page's. */
function platformBuild(): Plugin {
  const here = import.meta.dirname;
  return {
    name: "arugula-platform-build",
    apply: "build",
    generateBundle() {
      writeFileSync(resolve(here, "dist-platform.modules.json"), JSON.stringify([...this.getModuleIds()].sort(), null, 1));
    },
    writeBundle(o) {
      const out = o.dir ?? resolve(here, "dist-platform");
      renameSync(resolve(out, "app.html"), resolve(out, "index.html"));
      for (const f of ["favicon.ico", "icon.svg"]) copyFileSync(resolve(here, "public", f), resolve(out, f));
    },
  };
}

export default defineConfig({
  plugins: [platformBuild()],
  publicDir: false,
  define: { "import.meta.env.VITE_CONTROL_ORIGIN": JSON.stringify(process.env.VITE_CONTROL_ORIGIN ?? "") },
  // The page serves every route itself: an address Vite doesn't know is app.html.
  appType: "spa",
  server: {
    host: "127.0.0.1",
    port: Number(process.env.DEV_PLATFORM_PORT ?? 7691),
    strictPort: true,
    proxy: {
      // Control checks a session's Origin is its own.
      "/api": { target: control, changeOrigin: true, headers: { origin: control } },
    },
  },
  build: {
    outDir: "dist-platform",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: { input: { app: "app.html" } },
  },
});
