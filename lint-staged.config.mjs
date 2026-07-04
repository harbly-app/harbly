export default {
  // ESLint first (may rewrite code), Prettier second (formats the result).
  // Prettier respects .prettierignore even for explicitly passed files;
  // --no-warn-ignored keeps ESLint quiet about ignored files under --max-warnings 0.
  "*.{ts,tsx,js,mjs,astro}": [
    "eslint --fix --max-warnings 0 --no-warn-ignored",
    "prettier --write --ignore-unknown --ignore-path .prettierignore",
  ],
  "*.{css,html,json,md,yml,yaml}":
    "prettier --write --ignore-unknown --ignore-path .prettierignore",
  // Bare rustfmt picks up rustfmt.toml (edition) from the repo root.
  "*.rs": "rustfmt",
};
