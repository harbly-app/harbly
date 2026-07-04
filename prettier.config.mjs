// Style options are intentionally absent: we follow Prettier defaults.
/** @type {import("prettier").Config} */
export default {
  // tailwindcss plugin must stay last so it composes with the astro plugin.
  plugins: ["prettier-plugin-astro", "prettier-plugin-tailwindcss"],
  // Tailwind v4 has no JS config file; the plugin reads the CSS entry instead.
  tailwindStylesheet: "./apps/desktop/src/styles.css",
  overrides: [
    {
      files: "*.astro",
      options: { parser: "astro" },
    },
  ],
};
