import { defineConfig } from "vitest/config";

// Unit tests only (src/**/*.test.ts). Playwright owns e2e/**/*.spec.ts —
// without this include, vitest's default glob would try to execute the
// Playwright specs and crash on their test() registration.
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
  },
});
