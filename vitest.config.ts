import { defineConfig } from "vitest/config";

// apps/desktop gets its own vitest project in T0.3; until then the root run passes with no tests.
export default defineConfig({
  test: {
    include: ["apps/**/*.test.{ts,tsx}", "tools/**/*.test.ts"],
    passWithNoTests: true,
  },
});
