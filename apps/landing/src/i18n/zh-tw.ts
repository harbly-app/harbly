import type { Dict } from "./index";

// Traditional Chinese — translated from the zh-cn/en master copies.
const zhTw: Dict = {
  langName: "繁體中文",
  htmlLang: "zh-TW",
  title: "Harbly — 本地優先的 HTML 知識庫",
  description:
    "本地優先的單檔 HTML 知識庫：收藏網頁、撰寫筆記、全文搜尋，輸出一個檔案即可分享。免費開源，資料永遠在你自己的磁碟上。",
  nav: {
    features: "特色",
    how: "運作方式",
    download: "下載",
    soon: "即將推出",
  },
  chips: ["本地優先", "開源 AGPL-3.0", "macOS 首發"],
  hero: {
    h1Pre: "把知識，存成",
    h1Em: "永遠打得開",
    h1Post: "的網頁",
    lede: "開放、自包含、數十年後依然可讀——網頁是知識最可靠的容器。Harbly 圍繞單檔 HTML 打造本地知識庫：收藏、撰寫、整理、分享，資料永遠是你磁碟上的明文檔案。",
    ctaPrimary: "下載 macOS 版",
    ctaSecondary: "了解特色",
    trust: "免費 · 資料全在本地 · 明文檔案，解除安裝也帶得走",
  },
  mock: {
    aria: "Harbly 應用程式介面示意：左側為檔案夾樹，右側為資產網格",
    search: "搜尋資料庫內容  ⌘K",
    locations: "位置",
    inbox: "收件匣",
    folderActive: "專案文件",
    folderSub: "定價改版",
    folderB: "學習筆記",
    tagsLabel: "標籤",
    tagA: "重要",
    tagB: "靈感",
    cards: [
      {
        t: "讀書筆記·設計的心理學.hdoc",
        m: "學習筆記 · 昨天",
      },
      {
        t: "季度回顧儀表板.html",
        m: "專案文件 · 2 天前",
      },
      {
        t: "團隊週報.md",
        m: "專案文件 · 3 天前",
      },
      {
        t: "定價頁 A/B 草稿.html",
        m: "定價改版 · 上週",
      },
      {
        t: "京都旅遊攻略.html",
        m: "收件匣 · 剛剛",
      },
      {
        t: "排版實驗.html",
        m: "學習筆記 · 2 週前",
      },
    ],
  },
  features: {
    h2: "像 Finder，但更懂 HTML",
    sub: "你熟悉的檔案操作習慣全都保留，再補上知識庫需要的那一部分。",
    items: [
      {
        icon: "🗂️",
        title: "資料即檔案夾",
        desc: "知識庫就是磁碟上的一個普通檔案夾：在 Finder 整理，App 即時同步；明文檔案，git 或雲端硬碟都能管，解除安裝也帶得走。",
      },
      {
        icon: "✍️",
        title: "頁面文件與 Markdown",
        desc: "內建所見即所得的頁面文件與 Markdown 編輯器：筆記本身就是網頁，附三套主題，輸出即成品。",
      },
      {
        icon: "📤",
        title: "一個檔案就是分享",
        desc: "輸出單檔 HTML 傳給任何人：瀏覽器直接打開，免帳號、免安裝，離線也能看。",
      },
      {
        icon: "🛡️",
        title: "沙箱預覽，預設不連網",
        desc: "收進來的網頁放心打開：預覽注入 CSP，不發出任何網路請求；被攔下的外部連線逐筆計數，需要時單次放行、僅本次生效。",
      },
      {
        icon: "⌘K",
        kbd: true,
        title: "全文搜尋",
        desc: "SQLite FTS5 搭配中文斷詞，標題與內文全文命中；輸入即有結果，沒整理也找得回來。",
      },
      {
        icon: "🏷️",
        title: "Finder 標籤互通",
        desc: "標籤寫進檔案本身的 xattr,Finder 與 Spotlight 直接看得到；任一邊修改，另一邊自動跟上。",
      },
      {
        icon: "⌘Z",
        kbd: true,
        title: "還原與版本歷史",
        desc: "刪除、移動、重新命名、讀入，全部都能還原；每次編輯自動留下版本，隨時回到任何一稿。",
      },
      {
        icon: "🤖",
        title: "AI 助手，可選可換",
        desc: "本機 agent 免費用，或自帶 API key；讓 AI 改版、整理、產生頁面，成果自動進入版本歷史。",
      },
      {
        icon: "🌐",
        title: "六種語言介面",
        desc: "简体中文、繁體中文、English、日本語、한국어、Español——首次啟動跟隨系統語言，設定頁即時切換。",
      },
    ],
  },
  how: {
    h2: "三個步驟，把網頁變成知識庫",
    steps: [
      {
        title: "收與寫",
        desc: "拖曳收藏任何網頁，或直接新增頁面文件與 Markdown；內容雜湊自動去重，新項目先進收件匣。",
      },
      {
        title: "理與找",
        desc: "檔案夾、標籤、收藏隨手整理；⌘K 全文搜尋，沒整理也找得回來。",
      },
      {
        title: "傳出去",
        desc: "輸出單檔 HTML 傳給任何人，瀏覽器直接打開；資料從頭到尾都在你自己的磁碟上。",
      },
    ],
  },
  cta: {
    h2: "別把知識鎖在別人的 App 裡",
    sub: "開源免費，明文儲存，永遠帶得走——macOS 版即將開放下載。",
    btn: "下載 macOS 版",
  },
  footer: {
    tag: "本地優先的 HTML 知識庫",
    meta: "AGPL-3.0 開源 · © 2026 Harbly",
  },
};

export default zhTw;
