import type { Dict } from "./index";

// English — co-master copy (with zh-cn.ts) for all other locales.
const en: Dict = {
  langName: "English",
  htmlLang: "en",
  title: "Harbly — Local-first HTML knowledge base",
  description:
    "A local-first knowledge base built on single-file HTML. Clip web pages, write notes, search everything, and share by sending one file. Free, open source.",
  nav: {
    features: "Features",
    how: "How it works",
    download: "Download",
    soon: "coming soon",
  },
  chips: ["Local-first", "Open source · AGPL-3.0", "macOS first"],
  hero: {
    h1Pre: "Your knowledge, in the one format ",
    h1Em: "built to last",
    h1Post: "",
    lede: "Open, self-contained, and still readable decades from now — the web page is knowledge's most reliable container. Harbly is a local-first knowledge base built on single-file HTML: collect, write, organize, and share, with everything stored as plain files on your own disk.",
    ctaPrimary: "Download for macOS",
    ctaSecondary: "See the features",
    trust:
      "Free · All data stays local · Plain files you keep even after uninstalling",
  },
  mock: {
    aria: "Mockup of the Harbly app: folder tree on the left, asset grid on the right",
    search: "Search your library  ⌘K",
    locations: "Locations",
    inbox: "Inbox",
    folderActive: "Project docs",
    folderSub: "Pricing revamp",
    folderB: "Study notes",
    tagsLabel: "Tags",
    tagA: "Important",
    tagB: "Ideas",
    cards: [
      { t: "Reading notes · UX classics.hdoc", m: "Study notes · yesterday" },
      { t: "Quarterly review dashboard.html", m: "Project docs · 2 days ago" },
      { t: "Team weekly report.md", m: "Project docs · 3 days ago" },
      { t: "Pricing A/B draft.html", m: "Pricing revamp · last week" },
      { t: "Kyoto trip guide.html", m: "Inbox · just now" },
      { t: "Typography experiments.html", m: "Study notes · 2 weeks ago" },
    ],
  },
  features: {
    h2: "Like Finder, but fluent in HTML",
    sub: "Every file habit you already have, plus the parts a knowledge base needs.",
    items: [
      {
        icon: "🗂️",
        title: "Data is just a folder",
        desc: "Your library is a plain directory on disk: organize in Finder and the app follows instantly. Plain files that git or any cloud drive can manage — and that leave with you.",
      },
      {
        icon: "✍️",
        title: "Page documents & Markdown",
        desc: "Built-in WYSIWYG page documents and a Markdown editor: notes that are web pages themselves, with three themes — exporting gives you a finished page.",
      },
      {
        icon: "📤",
        title: "One file is the share",
        desc: "Export a single-file HTML and send it to anyone: it opens right in the browser — no account, no install, and it works offline.",
      },
      {
        icon: "🛡️",
        title: "Sandboxed preview, offline by default",
        desc: "Open collected pages without worry: previews get a strict CSP and make zero network requests; blocked external calls are counted, with one-time allow when you need it.",
      },
      {
        icon: "⌘K",
        kbd: true,
        title: "Full-text search",
        desc: "SQLite FTS5 with CJK-aware tokenization matches titles and body text; results as you type, even in an unsorted library.",
      },
      {
        icon: "🏷️",
        title: "Finder tag sync",
        desc: "Tags live in the file's own xattr, visible to Finder and Spotlight; change them on either side and the other follows.",
      },
      {
        icon: "⌘Z",
        kbd: true,
        title: "Undo & version history",
        desc: "Delete, move, rename, import — everything undoes. Every edit leaves a version you can roll back to at any time.",
      },
      {
        icon: "🤖",
        title: "Optional AI assistant",
        desc: "Use a free local agent or bring your own API key; let AI revise, organize, or generate pages — every change lands in version history.",
      },
      {
        icon: "🌐",
        title: "Six languages",
        desc: "简体中文, 繁體中文, English, 日本語, 한국어, Español — follows your system on first launch, switchable anytime.",
      },
    ],
  },
  how: {
    h2: "Three steps from web page to knowledge base",
    steps: [
      {
        title: "Collect & write",
        desc: "Drag in any web page, or create a page document or Markdown note; content hashing dedupes, and new items land in the inbox.",
      },
      {
        title: "Organize & find",
        desc: "Folders, tags, and favorites for tidying up; ⌘K full-text search finds things even when you don't.",
      },
      {
        title: "Share it on",
        desc: "Export a single HTML file and send it to anyone — it opens in the browser, while your data never leaves your disk.",
      },
    ],
  },
  cta: {
    h2: "Don't lock your knowledge inside someone else's app",
    sub: "Open source, plain-text storage, yours to keep — the macOS build is almost ready.",
    btn: "Download for macOS",
  },
  footer: {
    tag: "Local-first HTML knowledge base",
    meta: "Open source under AGPL-3.0 · © 2026 Harbly",
  },
};

export default en;
