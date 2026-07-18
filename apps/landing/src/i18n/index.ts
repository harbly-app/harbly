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

/** Per-locale webfont: Manrope everywhere, plus the matching Noto CJK family. */
export const FONTS: Record<Locale, { href: string; stack: string }> = {
  en: {
    href: "https://fonts.googleapis.com/css2?family=Manrope:wght@500;700;800&display=swap",
    stack: "Manrope, system-ui, -apple-system, sans-serif",
  },
  es: {
    href: "https://fonts.googleapis.com/css2?family=Manrope:wght@500;700;800&display=swap",
    stack: "Manrope, system-ui, -apple-system, sans-serif",
  },
  "zh-cn": {
    href: "https://fonts.googleapis.com/css2?family=Manrope:wght@500;700;800&family=Noto+Sans+SC:wght@400;500;700;900&display=swap",
    stack:
      'Manrope, "Noto Sans SC", system-ui, -apple-system, "PingFang SC", sans-serif',
  },
  "zh-tw": {
    href: "https://fonts.googleapis.com/css2?family=Manrope:wght@500;700;800&family=Noto+Sans+TC:wght@400;500;700;900&display=swap",
    stack:
      'Manrope, "Noto Sans TC", system-ui, -apple-system, "PingFang TC", sans-serif',
  },
  ja: {
    href: "https://fonts.googleapis.com/css2?family=Manrope:wght@500;700;800&family=Noto+Sans+JP:wght@400;500;700;900&display=swap",
    stack:
      'Manrope, "Noto Sans JP", system-ui, -apple-system, "Hiragino Kaku Gothic ProN", "Yu Gothic", sans-serif',
  },
  ko: {
    href: "https://fonts.googleapis.com/css2?family=Manrope:wght@500;700;800&family=Noto+Sans+KR:wght@400;500;700;900&display=swap",
    stack:
      'Manrope, "Noto Sans KR", system-ui, -apple-system, "Apple SD Gothic Neo", sans-serif',
  },
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
