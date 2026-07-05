import { defineConfig } from "vitest/config";

// jsdom: the hdoc parser needs a real DOMParser/TreeWalker; happy-dom's
// TreeWalker support is not complete enough.
export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
  },
});
