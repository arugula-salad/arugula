// The contact sheet's dev server (`pnpm sheet`): sheet/index.html, every
// character in every pose, for tuning them by eye.
import { defineConfig } from "vite";

export default defineConfig({
  root: import.meta.dirname,
  server: { port: 5199, host: "127.0.0.1", fs: { allow: [".."] } },
});
