// Landing i18n: locale registry mirroring the desktop app's six languages.
// The default locale (en) lives at "/", the rest under "/<locale>/".

export const LOCALES = ["en", "zh-cn", "zh-tw", "ja", "ko", "es"] as const;
export type Locale = (typeof LOCALES)[number];
export const DEFAULT_LOCALE: Locale = "en";

export interface Dict {
  langName: string;
  htmlLang: string;
  title: string;
  description: string;
  nav: { features: string; how: string; download: string; soon: string };
  chips: [string, string, string];
  hero: {
    h1Pre: string;
    h1Em: string;
    h1Post: string;
    lede: string;
    ctaPrimary: string;
    ctaSecondary: string;
    trust: string;
  };
  mock: {
    aria: string;
    search: string;
    locations: string;
    inbox: string;
    folderActive: string;
    folderSub: string;
    folderB: string;
    tagsLabel: string;
    tagA: string;
    tagB: string;
    cards: { t: string; m: string }[];
  };
  features: {
    h2: string;
    sub: string;
    items: { icon: string; kbd?: boolean; title: string; desc: string }[];
  };
  how: { h2: string; steps: { title: string; desc: string }[] };
  cta: { h2: string; sub: string; btn: string };
  footer: { tag: string; meta: string };
}

/** Per-locale font stacks. Manrope is self-hosted (latin only); CJK falls
 * through to first-class system fonts — no CJK webfont payload. */
export const FONT_STACKS: Record<Locale, string> = {
  en: "Manrope, system-ui, -apple-system, sans-serif",
  es: "Manrope, system-ui, -apple-system, sans-serif",
  "zh-cn":
    'Manrope, system-ui, -apple-system, "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif',
  "zh-tw":
    'Manrope, system-ui, -apple-system, "PingFang TC", "Microsoft JhengHei", sans-serif',
  ja: 'Manrope, system-ui, -apple-system, "Hiragino Kaku Gothic ProN", "Yu Gothic", Meiryo, sans-serif',
  ko: 'Manrope, system-ui, -apple-system, "Apple SD Gothic Neo", "Malgun Gothic", sans-serif',
};

/** Site path for a locale ("/" for the default, "/<locale>/" otherwise). */
export function localePath(locale: Locale): string {
  return locale === DEFAULT_LOCALE ? "/" : `/${locale}/`;
}

import en from "./en";
import zhCn from "./zh-cn";
import zhTw from "./zh-tw";
import ja from "./ja";
import ko from "./ko";
import es from "./es";

export const DICTS: Record<Locale, Dict> = {
  en,
  "zh-cn": zhCn,
  "zh-tw": zhTw,
  ja,
  ko,
  es,
};
