import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    // One jsdom per worker, still a fresh VM context (module graph + globals) per file:
    // the default pool builds a new jsdom for every file, ~35% of the run.
    pool: "vmThreads",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
    // Unit tests live under src/; e2e/ is Playwright's (its own runner).
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
