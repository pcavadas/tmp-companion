import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    // One jsdom per worker, still a fresh VM context (module graph + globals) per file:
    // the default pool built a new jsdom for each of ~90 files, ~35% of the run.
    // Measured 2026-10-13: 76 s → 36 s, the whole suite green on repeated runs.
    pool: "vmThreads",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
    // Unit tests live under src/; e2e/ is Playwright's (its own runner).
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
