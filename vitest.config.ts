import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
    // Unit tests live under src/; e2e/ is Playwright's (its own runner).
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
