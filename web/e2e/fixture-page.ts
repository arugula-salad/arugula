// A fixture page (a .tsx beside the specs) built with Vite into one HTML
// document, script and style inline, so no daemon serves it: a spec routes
// its address to it.
import { fileURLToPath } from "node:url";
import { build } from "vite";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

type Out = { type: string; fileName: string; code?: string; source?: string | Uint8Array };
const built = new Map<string, Promise<string>>();

/** `entry`'s HTML, built once per worker. */
export function fixturePage(entry: string): Promise<string> {
  const path = fileURLToPath(new URL(entry, import.meta.url));
  if (!built.has(path)) {
    built.set(
      path,
      (async () => {
        const out = (await build({
          root: ROOT,
          configFile: false,
          oxc: { jsx: { runtime: "automatic", importSource: "preact" } },
          logLevel: "silent",
          build: {
            write: false,
            target: "es2022",
            lib: { entry: path, formats: ["es"], fileName: "page" },
            cssCodeSplit: false,
            // The emoji table is a dynamic import: one script has it too.
            rollupOptions: { output: { inlineDynamicImports: true } },
          },
        })) as { output: Out[] } | { output: Out[] }[];
        const files = Array.isArray(out) ? out.flatMap((o) => o.output) : out.output;
        const js = files.find((f) => f.type === "chunk")?.code ?? "";
        const css = files
          .filter((f) => f.fileName.endsWith(".css"))
          .map((f) => String(f.source))
          .join("\n");
        return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><style>${css}</style></head><body><div id="root"></div><script type="module">${js.replaceAll("</script", "<\\/script")}</script></body></html>`;
      })(),
    );
  }
  return built.get(path)!;
}
