import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://harbly.app",
  integrations: [
    sitemap({
      filter: (page) => !page.includes("/404"),
      i18n: {
        defaultLocale: "en",
        locales: {
          en: "en",
          "zh-cn": "zh-CN",
          "zh-tw": "zh-TW",
          ja: "ja",
          ko: "ko",
          es: "es",
        },
      },
    }),
  ],
});
