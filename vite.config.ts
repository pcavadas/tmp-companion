import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Port 1421 (emulator owns 1420) so both apps' dev servers can coexist. The e2e runner
// overrides it via TMP_E2E_VITE_PORT for per-worktree isolation (see scripts/e2e.sh).
const port = Number(process.env.TMP_E2E_VITE_PORT) || 1421;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "safari15",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
