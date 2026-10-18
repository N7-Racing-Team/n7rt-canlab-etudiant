import { defineConfig } from "vite";
import { readFileSync } from "node:fs";
const version = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version;

// Tauri expects a fixed dev port and serves the build from ../dist (see src-tauri/tauri.conf.json).
export default defineConfig({
  root: "ui",
  clearScreen: false,
  define: { __APP_VERSION__: JSON.stringify(version) },
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/target/**"] } },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: false,
  },
});
