# Harbly 技术栈设计

v1.0 · 2026-07 · 配套文档：《产品 Roadmap》《本地版 UI 设计文档》
适用范围：P0（本地核心）与 P1（收藏入口），并为 P2+ 云端留出接口边界。

---

## 一、结论一览

| 层               | 选型                                                                                                 | 一句话理由                                                                                     |
| ---------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| 桌面壳           | **Tauri 2.x**（Rust）                                                                                | 常驻后台的轻量工具，体积/内存远优于 Electron；文件重活正好是 Rust 强项                         |
| 前端             | **React 19 + TypeScript + Vite + Tailwind CSS 4**                                                    | 生态最全：cmdk（⌘K）、react-arborist（目录树拖拽）、TanStack Virtual（千级缩略图网格）现成可用 |
| 状态管理         | **Zustand**                                                                                          | 轻量，够用，无模板代码                                                                         |
| 核心逻辑         | **Rust 独立 crate `harbly-core`**                                                                    | 索引/哈希/版本串/搜索与 UI 解耦，可被 App、CLI、MCP 三端复用，单测友好                         |
| 索引数据库       | **SQLite（rusqlite, bundled）+ FTS5**                                                                | 单文件、零部署，存于库内 `.harbly/index.db`                                                    |
| 中文全文搜索     | **jieba-rs 预分词 + FTS5 unicode61**                                                                 | 纯 Rust，解决 FTS5 对中文二字词（如"定价"）不可搜的问题                                        |
| 内容哈希         | **BLAKE3**                                                                                           | 去重与元数据主键，速度远超 SHA-256                                                             |
| 文件监视         | **notify crate**（macOS 走 FSEvents）                                                                | 监视下载文件夹 + 库目录外部改动检测，同一套机制                                                |
| 删除             | **trash crate**                                                                                      | 跨平台移入系统废纸篓，符合设计文档要求                                                         |
| 密钥存储         | **keyring crate**                                                                                    | BYOK 的 API Key 存 macOS 钥匙串，不落盘                                                        |
| AI 供给          | Rust 侧 `AiProvider` trait：本地 Agent（spawn Claude Code CLI）/ BYOK（reqwest + SSE）/ Cloud（P2+） | 三供给同一接口，随时切换，权限边界在 Rust 层强制                                               |
| MCP / CLI        | **单一 `harbly` 二进制**，子命令 `harbly mcp`（官方 rmcp SDK, stdio）                                | agent 产物自动入库；同一二进制兼任浏览器插件的本地桥                                           |
| 浏览器插件（P1） | **WebExtension（TS, Manifest V3）↔ 本地 127.0.0.1 HTTP + token**                                     | Zotero Connector 同款验证过的模式，免每浏览器装 native messaging manifest                      |
| 首发平台         | **macOS**（10.15+），代码保持跨平台整洁                                                              | 设计文档全按 macOS 惯例（Finder/废纸篓/⌘）                                                     |
| 许可             | AGPL（应用）；全部依赖 MIT/Apache 兼容                                                               | 与 Roadmap 的开源承诺一致                                                                      |

---

## 二、为什么是 Tauri（而不是 Electron）

**支持 Tauri 的硬理由：**

1. **产品形态是"常驻伴侣应用"**——监视文件夹、接收插件收藏、做 MCP server，需要长期驻留。Tauri 安装包 ~10MB / 空闲内存几十 MB，Electron 基线 ~150MB+。对开源分发（P0 出场门槛是 GitHub 100 星）小包体是实打实的转化率。
2. **后端工作全是 Rust 甜区**：文件监视（notify）、内容哈希（blake3）、SQLite/FTS、进程管理（spawn agent CLI）、废纸篓、钥匙串——每一项都有成熟 crate，质量普遍高于 Node 等价物。
3. **MCP Server + CLI 可以和核心逻辑同仓同语言**：`harbly-core` 一个 crate，App/CLI/MCP 三端复用，不需要 Node sidecar。

**Tauri 的两个已知代价与对策：**

| 风险                                            | 说明                                                | 对策                                                                                                                                                                                                                                                                                                                                                   |
| ----------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **无头缩略图截屏**（P0 需求"后台无头渲染截图"） | Electron 有一行 `capturePage()`；Tauri 没有内置 API | 三平台都有原生 snapshot API：macOS `WKWebView.takeSnapshot`、Windows `WebView2.CapturePreviewAsync`、Linux `webkit_web_view_get_snapshot`。Tauri 2 的 `with_webview` 可拿到原生 webview 句柄，macOS 用 objc2 调 takeSnapshot（隐藏窗口加载 + 5s 超时 → 失败降级为代码图标，正好对应设计稿的失败态）。封装成 `trait ThumbnailRenderer`，P0 只实现 macOS |
| **渲染引擎是 WKWebView（Safari 内核）**         | 资产在 app 内预览效果可能与 Chrome 有细微差异       | 资产本来就是"给浏览器看的文件"，WKWebView 足够现代；"在浏览器打开"是设计内置的逃生口。收益换代价：值得                                                                                                                                                                                                                                                 |

其余常见顾虑不成立：沙箱预览靠 iframe sandbox + CSP，是 Web 标准层面的事，与壳无关；tauri-driver 不支持 macOS 的 e2e 限制，用"核心逻辑下沉 Rust 单测 + 前端 Playwright"绕开（见 §八）。

---

## 三、架构总览

```
┌────────────────────────────────────────────────────────┐
│  React 前端（Vite + Tailwind + Zustand）                 │
│  网格/列表 · 目录树 · 收件箱 · 查看器 · ⌘K · AI 面板 · 直改  │
│         │ Tauri IPC（commands + Channel 流式事件）        │
├─────────┴──────────────────────────────────────────────┤
│  src-tauri（壳层）：窗口 · 菜单 · 托盘 · 自定义协议 ·        │
│  隐藏缩略图窗口 · 插件（updater/deep-link/single-instance） │
├────────────────────────────────────────────────────────┤
│  harbly-core（纯 Rust crate，无 UI 依赖）                 │
│  库管理 · 扫描/聚链 · 哈希去重 · 版本串 · FTS 索引 ·         │
│  文件监视 · 收件箱规则 · AiProvider trait · 权限边界        │
├────────────────────────────────────────────────────────┤
│  harbly-cli（同 workspace 二进制）                        │
│  harbly add/list/export · harbly mcp（stdio, rmcp）·     │
│  127.0.0.1 HTTP 桥（浏览器插件用，token 鉴权）              │
└────────────────────────────────────────────────────────┘
磁盘布局（= 产品承诺"数据即文件夹"）：
~/Harbly/                    ← 用户可见明文文件，Finder/git 可管
  客户项目/星链SaaS/xxx.html
  _inbox/                    ← 收件箱
  .harbly/                   ← 应用私有，不污染内容
    index.db                 ← SQLite：元数据(按内容哈希键) + FTS5
    versions/<asset-id>/v1.html … vN.html   ← 版本全量文件
    thumbs/<hash>.jpg        ← 缩略图缓存
```

Cargo workspace：`crates/harbly-core` / `apps/desktop/src-tauri`（P1 增 `harbly-cli`）；pnpm workspace：`apps/desktop`（桌面前端）/ `apps/landing`（Astro 官网）/ P1 增 `extension`（插件）。根 package.json 仅做编排：`dev:desktop` / `dev:landing` / `build:*` / `test:core`。

---

## 四、关键需求 → 技术方案

### 1. 沙箱预览（P0，安全是卖点）

- 注册自定义协议 `harbly-asset://`（`register_uri_scheme_protocol`），从库读文件返回响应，**响应头注入 CSP**：`default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'` —— 允许内联脚本样式（artifact 常态），封死一切网络。
- 预览 iframe：`sandbox="allow-scripts allow-same-origin"`。安全性来自**协议即源隔离**：`harbly-asset://` 与应用壳 `tauri://` 天然跨源，framed 页面碰不到宿主；`allow-same-origin` 只为让资产自己的 localStorage 不炸。不给 `allow-popups` / `allow-top-navigation`。
- "外链已拦截 N 项"：iframe 内注入监听 `securitypolicyviolation` 事件，计数上报宿主 UI。**一次性放行** = 以放行域名重新生成 CSP 串并刷新预览，不持久化。
- 后续可选（P1+ 产品决策）：本地缓存常见 CDN 库（tailwind/react/chart.js），断网也能完整渲染。

### 2. 缩略图管线（P0）

- 隐藏 `WebviewWindow`（`visible(false)`）加载 `harbly-asset://` 地址 → 等 load + 800ms 稳定期 → 原生 snapshot（macOS: objc2 调 `WKWebView.takeSnapshot`）→ `image` crate 缩放编码 JPEG → `.harbly/thumbs/<hash>.jpg`。
- 队列串行 + 5s 超时；超时/脚本报错 → 记失败态（对应设计稿"脚本超时 · 显示代码图标 · 点击重试"）。首次启动扫描时后台批跑（对应引导页"生成缩略图 412/647"）。

### 3. 版本串与去重（P0）

- 主键 = BLAKE3 内容哈希；导入时命中已有哈希 → 秒去重。
- 版本 = 全量文件存 `.harbly/versions/`（设计文档明文要求，空间换简单可靠）；SQLite 记 `asset → versions` 链、来源（Claude/ChatGPT/Gemini/插件/监视/直改/AI 改版）、时间。
- 元数据按哈希不按路径 → 用户在 Finder 里移动/改名不丢历史（监视器负责把新路径重新绑定）。
- 相似度聚链（收件箱"92% 相似建议归入版本串"）：P0 用剥标签后的 SimHash/MinHash 文本相似度，纯本地零成本；语义 embedding 留到 P4。

### 4. 目录双向同步与冲突（P0）

- `notify`（FSEvents）监视整个库：外部新增/改动/移动/删除 → 增量更新索引与缩略图。
- 正在预览的文件被外部修改 → 弹设计稿定义的横幅（刷新预览 / 存外部版为 vN+1）；**外部改动永不覆盖版本串**，检测到差异只追加。
- 应用内拖拽移动 = 真实 `mv`（对应设计稿"拖拽即磁盘 mv"）。

### 5. 中文全文搜索（P0）

- FTS5 默认 tokenizer 对 CJK 不分词、trigram 又搜不了二字词（设计稿演示搜"定价"）→ 方案：**索引与查询两侧都过 jieba-rs 分词**，分词结果存 FTS5 影子表（unicode61），英文/代码天然兼容。
- 索引内容 = 剥标签正文 + 文件名 + 标签 + 来源元数据。⌘K 的"资产命中（标题+全文）"直接查 FTS，命令与 AI 转发在前端路由。

### 6. 文字直改（P1，逻辑在前端）

- 预览中双击文本 → contenteditable 单节点编辑。
- 保存回写：**parse5**（`sourceCodeLocationInfo: true`）在原始源码中定位静态 DOM 文本节点 → 字符串精准 splice，布局样式零扰动；脚本生成的文本走源码字面量匹配，匹配不到 → 冻结快照存为新版本（设计文档已定义此降级链）。
- 版本对比"2 处文字差异"：剥标签文本 + jsdiff，双 iframe 并排渲染。

### 7. AI 三供给（P0 接口 / P1-P4 逐步实现）

```rust
trait AiProvider {
    fn chat_stream(&self, ctx: AssetContext, msgs: …) -> impl Stream<Item = Delta>;
    // P4: fn embed(&self, …)
}
```

- **本地 Agent**（默认，免费）：探测 `claude` 可执行文件 → spawn `claude -p --output-format stream-json`，stdout 流经 Tauri Channel 推给前端；Harbly 自身以 `harbly mcp` 暴露库操作（搜索/读取/写新版本），agent 产物自动回到版本串。Codex CLI 同接口适配。
- **BYOK**：reqwest + SSE 直连 Anthropic/OpenAI，Key 存 keyring（钥匙串）。
- **Cloud**：P2+ 再实现，接口预留。
- **权限边界在 harbly-core 强制**（不是 UI 约定）：`AI 可读内容` 开关决定上下文注入；一切写操作 → 生成 diff → 前端确认 → 才落新版本。

### 8. 收藏入口（P1）

- **浏览器插件**：WebExtension（TS + Manifest V3），content script 提取 ChatGPT/Claude/Gemini 页面的 artifact HTML 与来源元数据（平台/对话/提示词）→ POST 到本机 `127.0.0.1:<port>`（首启握手发 token，Zotero Connector 模式）；App 未运行时降级 `harbly://` deep-link 唤起。
- **监视下载文件夹**：notify 盯 `~/Downloads`，出现 `.html` → 收进 `_inbox` 并登记来源。
- **剪贴板捕获**：`clipboard-manager` 插件轮询检测 HTML 片段 → 一键存资产。
- **MCP / CLI**：见 §三，`harbly mcp` 一条命令接入 Claude Code。

---

## 五、Tauri 插件与系统集成清单

| 需求                        | 插件/方案                                                            |
| --------------------------- | -------------------------------------------------------------------- |
| 在 Finder 显示 / 浏览器打开 | tauri-plugin-opener（`reveal_item_in_dir` / `open_url`）             |
| 单实例 + 插件唤起           | tauri-plugin-single-instance + tauri-plugin-deep-link（`harbly://`） |
| 自动更新                    | tauri-plugin-updater（DMG + Homebrew cask 分发）                     |
| 开机自启（监视常驻）        | tauri-plugin-autostart（默认关，设置里开）                           |
| 拖入导入                    | Tauri 内置 onDragDropEvent（拿真实路径）                             |
| 拖出到 Finder               | @crabnebula/tauri-plugin-drag                                        |
| 全局快捷键 / 通知 / 日志    | global-shortcut · notification · tauri-plugin-log（tracing 后端）    |
| 进程 spawn（agent CLI）     | tauri-plugin-shell（或 core 内 tokio::process）                      |

---

## 六、前端依赖清单

| 用途                           | 库                                                                   |
| ------------------------------ | -------------------------------------------------------------------- |
| ⌘K 命令面板                    | cmdk                                                                 |
| 目录树（拖拽/重命名/内联编辑） | react-arborist                                                       |
| 千级网格/列表虚拟化            | @tanstack/react-virtual                                              |
| 菜单/对话框/弹层无障碍原语     | Radix UI Primitives                                                  |
| 图标                           | lucide-react                                                         |
| 文本 diff                      | jsdiff                                                               |
| 直改源码映射                   | parse5                                                               |
| 样式                           | Tailwind CSS 4（设计令牌 #6E56CF 等进 `@theme`；深色模式后续零成本） |

字体 Manrope + Noto Sans SC 本地打包（应用自身也遵守"默认断网"人设）。

---

## 七、Rust 依赖清单

tokio · serde/serde_json · rusqlite（bundled）+ rusqlite_migration · blake3 · notify · trash · keyring · jieba-rs · reqwest（stream）· rmcp（官方 MCP SDK）· image · similar（diff）· thiserror/anyhow · tracing · objc2 系列（macOS snapshot 桥）。全部 MIT/Apache，与 AGPL 应用许可兼容。

---

## 八、工程与质量

- **工具链**：Rust stable · Node 22 LTS · pnpm · Xcode CLT。
- **测试策略**（tauri-driver 不支持 macOS，绕开壳层 e2e）：
  - harbly-core：cargo test 重点覆盖版本串、去重、聚链、FTS、冲突规则——最有价值的逻辑全在这层；
  - 前端：Vitest + Testing Library；关键流程（收件箱三分支、直改保存）用 Playwright 打 Vite dev server（IPC mock）；
  - 壳层：CI 三平台构建冒烟 + 手动清单。
- **CI/CD**：GitHub Actions（macOS 构建 + tauri-action 出 release）；发布前需 Apple Developer ID 签名+公证（$99/年，P0 公开发布前解决）。
- **代码风格**：rustfmt + clippy；ESLint + Prettier。

---

## 九、开放问题（待产品/后续决定）

1. **多库切换**（设计稿未覆盖）：app 级配置先按 `libraries: []` 数组建模，UI 后补，避免返工。
2. **CDN 依赖的离线渲染**：是否本地缓存常见库供放行时使用（影响缩略图成功率）。
3. **Windows/Linux 时间点**：架构保持跨平台（snapshot 已抽 trait），但 P0 只对 macOS 做体验验收。
4. **公证账号与开源仓库名**：产品名已定 **Harbly**（2026-07，面向用户名称）；2026-07-04 起内部标识符全面随品牌统一为 `harbly-*`——crate `harbly-core`/`harbly-app`、协议 `harbly-asset://`/`harbly-thumb://`、库私有目录 `.harbly/`、bundle id `com.zeyu.harbly`、localStorage `harbly.*`。未上架阶段不留任何迁移/兼容代码（2026-07-04 裁决）。GitHub 仓库名与 crate/插件命名空间发布前最终确认。
