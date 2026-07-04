import { defineConfig, globalIgnores } from "eslint/config";
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import astro from "eslint-plugin-astro";
import prettier from "eslint-config-prettier";
import globals from "globals";

export default defineConfig([
  // Mirrors .prettierignore scope; ESLint deliberately does not follow
  // .gitignore so the temporarily git-ignored apps/landing is still linted.
  globalIgnores([
    "**/dist/",
    "**/target/",
    "apps/desktop/src-tauri/gen/",
    "docs/",
    ".claude/",
    "**/.astro/",
    "**/.vite/",
  ]),

  // TypeScript with type-aware rules (strict: the codebase is young, keep the bar high)
  {
    files: ["**/*.{ts,tsx}"],
    extends: [
      js.configs.recommended,
      tseslint.configs.strictTypeChecked,
      tseslint.configs.stylisticTypeChecked,
    ],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
      globals: globals.browser,
    },
    rules: {
      // Idiomatic React/zustand arrow shorthand: onClick={() => set(...)}
      "@typescript-eslint/no-confusing-void-expression": [
        "error",
        { ignoreArrowShorthand: true },
      ],
      // Async event handlers on void-returning JSX props are fine (official
      // typescript-eslint recommendation for React codebases).
      "@typescript-eslint/no-misused-promises": [
        "error",
        { checksVoidReturn: { attributes: false } },
      ],
      // Arrow noops as defaults/placeholders: () => {}
      "@typescript-eslint/no-empty-function": [
        "error",
        { allow: ["arrowFunctions"] },
      ],
      // Numbers interpolate into UI strings constantly (counts, sizes, dates)
      "@typescript-eslint/restrict-template-expressions": [
        "error",
        { allowNumber: true },
      ],
    },
  },

  // React (desktop app)
  {
    files: ["apps/desktop/src/**/*.{ts,tsx}"],
    extends: [reactHooks.configs.flat.recommended, reactRefresh.configs.vite],
    rules: {
      // React Compiler is not enabled; library-compat advisories are noise here
      "react-hooks/incompatible-library": "off",
    },
  },

  // Astro (landing)
  astro.configs.recommended,

  // Plain JS config files (this file, commitlint, prettier, lint-staged, astro.config)
  {
    files: ["**/*.{js,mjs}"],
    extends: [js.configs.recommended],
    languageOptions: { globals: globals.node },
  },

  // Must stay last: disables anything that would fight Prettier
  prettier,
]);
