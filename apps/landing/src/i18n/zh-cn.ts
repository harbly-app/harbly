import type { Dict } from "./index";

// Simplified Chinese — co-master copy (with en.ts) for all other locales.
const zhCn: Dict = {
  langName: "简体中文",
  htmlLang: "zh-CN",
  title: "Harbly — 本地优先的 HTML 知识库",
  description:
    "本地优先的单文件 HTML 知识库：收藏网页、撰写笔记、全文搜索，导出一个文件即可分享。免费开源，数据永远在你自己的磁盘上。",
  nav: {
    features: "特性",
    how: "工作方式",
    download: "下载",
    soon: "即将发布",
  },
  chips: ["本地优先", "开源 AGPL-3.0", "macOS 首发"],
  hero: {
    h1Pre: "把知识，存成",
    h1Em: "永远打得开",
    h1Post: "的网页",
    lede: "开放、自包含、几十年不腐——网页是知识最可靠的容器。Harbly 围绕单文件 HTML 打造本地知识库：收藏、撰写、整理、分享，数据永远是你磁盘上的明文文件。",
    ctaPrimary: "下载 macOS 版",
    ctaSecondary: "了解特性",
    trust: "免费 · 数据全在本地 · 明文文件，卸载也带得走",
  },
  mock: {
    aria: "Harbly 应用界面示意：左侧目录树，右侧资产网格",
    search: "搜索库内容  ⌘K",
    locations: "位置",
    inbox: "收件箱",
    folderActive: "项目文档",
    folderSub: "定价改版",
    folderB: "学习笔记",
    tagsLabel: "标签",
    tagA: "重要",
    tagB: "灵感",
    cards: [
      { t: "读书笔记·设计心理学.hdoc", m: "学习笔记 · 昨天" },
      { t: "季度复盘仪表盘.html", m: "项目文档 · 2 天前" },
      { t: "团队周报.md", m: "项目文档 · 3 天前" },
      { t: "定价页 A/B 草稿.html", m: "定价改版 · 上周" },
      { t: "旅行攻略·京都.html", m: "收件箱 · 刚刚" },
      { t: "排版实验.html", m: "学习笔记 · 2 周前" },
    ],
  },
  features: {
    h2: "像 Finder，但懂 HTML",
    sub: "文件操作的每个习惯都保留，再补上知识库需要的那部分。",
    items: [
      {
        icon: "🗂️",
        title: "数据即文件夹",
        desc: "知识库就是磁盘上的普通目录：Finder 里整理，应用即时同步；明文文件，git / 网盘可管，卸载也带得走。",
      },
      {
        icon: "✍️",
        title: "页面文档与 Markdown",
        desc: "内置所见即所得的页面文档与 Markdown 编辑器：笔记本身就是网页，三套主题，导出即成品。",
      },
      {
        icon: "📤",
        title: "一个文件就是分享",
        desc: "导出单文件 HTML 发给任何人：浏览器直接打开，无需账号、无需安装，离线也能看。",
      },
      {
        icon: "🛡️",
        title: "沙箱预览，默认断网",
        desc: "收进来的网页放心打开：预览注入 CSP，不发一个网络请求；外链拦截逐条计数，需要时一次放行一次生效。",
      },
      {
        icon: "⌘K",
        kbd: true,
        title: "全文搜索",
        desc: "SQLite FTS5 加中文分词，标题与正文全文命中；键入即得，不整理也能找回。",
      },
      {
        icon: "🏷️",
        title: "Finder 标签互通",
        desc: "标签写进文件本身的 xattr，Finder 与 Spotlight 直接可见；任意一侧修改，另一侧自动跟上。",
      },
      {
        icon: "⌘Z",
        kbd: true,
        title: "撤销与版本历史",
        desc: "删除、移动、重命名、导入全部可撤销；每次编辑自动留下版本，随时回滚到任何一稿。",
      },
      {
        icon: "🤖",
        title: "AI 助手，可选可换",
        desc: "本地 agent 免费用，或自带 API key；让 AI 改版、整理、生成页面，产出自动进入版本历史。",
      },
      {
        icon: "🌐",
        title: "六语言界面",
        desc: "简体中文、繁體中文、English、日本語、한국어、Español——首启跟随系统，设置页即时切换。",
      },
    ],
  },
  how: {
    h2: "三步，把网页变成知识库",
    steps: [
      {
        title: "收与写",
        desc: "拖拽收藏任何网页，或直接新建页面文档与 Markdown；内容哈希去重，新入先进收件箱。",
      },
      {
        title: "理与找",
        desc: "文件夹、标签、收藏夹随手整理；⌘K 全文搜索，不整理也找得回。",
      },
      {
        title: "发出去",
        desc: "导出单文件 HTML 发给任何人，浏览器直接打开；数据始终在你自己的磁盘上。",
      },
    ],
  },
  cta: {
    h2: "别把知识锁在别人的应用里",
    sub: "开源免费，明文存储，永远带得走——macOS 版即将开放下载。",
    btn: "下载 macOS 版",
  },
  footer: {
    tag: "本地优先的 HTML 知识库",
    meta: "AGPL-3.0 开源 · © 2026 Harbly",
  },
};

export default zhCn;
