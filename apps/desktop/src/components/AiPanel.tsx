import {
  ArrowUp,
  Check,
  ChevronDown,
  FileDiff,
  History,
  Plus,
  Settings2,
  Sparkles,
  Square,
  Trash2,
  Undo2,
  X,
} from "lucide-react";
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import Markdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, timeAgo } from "../lib/api";
import { localizeAiError, localizeVerLabel, makeT } from "../lib/i18n";
import type { TFn } from "../lib/i18n";
import { useStore } from "../lib/store";
import type {
  AgentInfo,
  AiConfig,
  AiEffort,
  AiMessage,
  AiRun,
  AiSession,
  AiSupply,
  VersionInfo,
} from "../lib/types";

/** One selectable supply in the prefs popover */
interface SupplyOption {
  id: AiSupply;
  label: string;
  local: boolean;
}

interface LiveRun {
  job: string;
  /** Which conversation this run belongs to — the panel can switch away
   * mid-stream, and the deltas must not leak into another transcript. */
  sessionId: string;
  instruction: string;
  actions: string[];
  text: string;
}

/** Draft prefs for a not-yet-created conversation */
interface Prefs {
  supply: AiSupply | null;
  model: string;
  effort: AiEffort;
}

const LAST_SESSION_KEY = "harbly.aiSession";

function supplyLabel(id: AiSupply): string {
  switch (id) {
    case "claude":
      return "Claude Code";
    case "codex":
      return "Codex CLI";
    case "anthropic":
      return "Anthropic API";
    case "openai":
      return "OpenAI API";
    case "openrouter":
      return "OpenRouter";
  }
}

function buildOptions(
  agents: AgentInfo[],
  keys: Record<string, boolean>,
): SupplyOption[] {
  const out: SupplyOption[] = agents.map((a) => ({
    id: a.kind,
    label: supplyLabel(a.kind),
    local: true,
  }));
  for (const p of ["anthropic", "openai", "openrouter"] as const) {
    if (keys[p]) out.push({ id: p, label: supplyLabel(p), local: false });
  }
  return out;
}

/** The AI panel: a library-scoped conversation host. Sessions persist in the
 * library; the currently open file rides along as context. All file access
 * happens through the shared tool surface, so every write in a transcript is
 * a version somewhere. */
export default function AiPanel() {
  const lang = useStore((s) => s.lang);
  const t = makeT(lang);
  const toggleAi = useStore((s) => s.toggleAi);
  const showToast = useStore((s) => s.showToast);
  const viewerAsset = useStore((s) => s.viewerAsset);
  const aiConfigEpoch = useStore((s) => s.aiConfigEpoch);
  const root = useStore((s) => s.root);
  // Sessions live in the library database — remember the selection per library
  const sessionKey = `${LAST_SESSION_KEY}:${root ?? ""}`;

  const [options, setOptions] = useState<SupplyOption[] | null>(null);
  const [config, setConfig] = useState<AiConfig>({});
  const [sessions, setSessions] = useState<AiSession[]>([]);
  const [activeId, setActiveId] = useState<string | null>(() =>
    localStorage.getItem(sessionKey),
  );
  const [messages, setMessages] = useState<AiMessage[]>([]);
  const [draft, setDraft] = useState<Prefs>({
    supply: null,
    model: "",
    effort: "",
  });
  const [input, setInput] = useState("");
  const [live, setLive] = useState<LiveRun | null>(null);
  const [menu, setMenu] = useState<"none" | "sessions" | "prefs" | "versions">(
    "none",
  );
  const [versions, setVersions] = useState<VersionInfo[]>([]);
  const [runs, setRuns] = useState<AiRun[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const liveRef = useRef<LiveRun | null>(null);
  const activeIdRef = useRef(activeId);
  // Monotonic ticket for message fetches — a slow response for a previously
  // viewed session must not overwrite the transcript on screen
  const msgEpoch = useRef(0);
  // Closes the async gap in send() before `live` exists (draft creation):
  // Enter auto-repeat there would create two sessions and two turns
  const sendingRef = useRef(false);
  // WKWebView delivers the composition-confirming Enter AFTER compositionend
  // with isComposing already false — remember when composition last ended
  const composeEndAt = useRef(0);
  // Session id whose transcript load should be skipped once: the draft-send
  // path selects the just-created session, and the effect's fetch would race
  // the backend's user-row append and blank the optimistic bubble
  const skipLoadRef = useRef<string | null>(null);
  // Autoscroll only while the user is already at the bottom
  const stickRef = useRef(true);

  const active = sessions.find((s) => s.id === activeId) ?? null;
  // Effective prefs: the active session's, or the draft's for a new one
  const prefs: Prefs = active
    ? { supply: active.supply, model: active.model, effort: active.effort }
    : draft;
  const empty = options !== null && options.length === 0;

  const loadSupplies = useCallback(() => {
    return Promise.all([
      api.aiDetectAgents().catch((): AgentInfo[] => []),
      api.aiKeyStatus().catch((): Record<string, boolean> => ({})),
      api.aiGetConfig().catch((): AiConfig => ({})),
    ]).then(([agents, keys, cfg]) => {
      const opts = buildOptions(agents, keys);
      setOptions(opts);
      setConfig(cfg);
      setDraft((d) => {
        const want = d.supply ?? cfg.supply ?? null;
        const first: AiSupply | null = opts.length > 0 ? opts[0].id : null;
        const supply =
          want != null && opts.some((o) => o.id === want) ? want : first;
        if (supply == null) return { ...d, supply };
        // A draft always carries a concrete model + effort (no default tier)
        return d.supply === supply && d.model !== ""
          ? d
          : { supply, ...initialPrefsFor(supply) };
      });
    });
  }, []);

  const loadSessions = useCallback(() => {
    const startedActive = activeIdRef.current;
    return api
      .aiSessionsList()
      .then((list) => {
        setSessions(list);
        // Reconcile a stale remembered id (deleted session, reset database):
        // a phantom active id would fail every send with 会话不存在. Skip if
        // the selection changed while fetching (a draft was just created).
        setActiveId((cur) =>
          cur && cur === startedActive && !list.some((s) => s.id === cur)
            ? null
            : cur,
        );
      })
      .catch(() => {});
  }, []);

  const loadMessages = useCallback((id: string | null) => {
    const ticket = ++msgEpoch.current;
    const fetching: Promise<AiMessage[]> = id
      ? api.aiSessionMessages(id).catch((): AiMessage[] => [])
      : Promise.resolve([]);
    return fetching.then((msgs) => {
      // Only the newest request may paint — invoke responses can reorder
      // when sessions are switched quickly
      if (msgEpoch.current === ticket) setMessages(msgs);
    });
  }, []);

  // Versions + runs of the file being viewed: feeds the history popover and
  // the per-message version cards
  const loadAssetMeta = useCallback(() => {
    const id = viewerAsset?.id;
    const fetching: Promise<[VersionInfo[], AiRun[]]> = id
      ? Promise.all([
          api.listVersions(id).catch((): VersionInfo[] => []),
          api.aiRunsList(id).catch((): AiRun[] => []),
        ])
      : Promise.resolve([[], []]);
    void fetching.then(([vs, rs]) => {
      setVersions(vs);
      setRuns(rs);
    });
  }, [viewerAsset]);

  useEffect(() => {
    void loadSupplies();
  }, [loadSupplies, aiConfigEpoch]);

  // Mount AND library switch: fetch this library's sessions and re-seed the
  // selection from its remembered key — keeping another library's list would
  // offer ghost conversations that fail every send.
  useEffect(() => {
    const remembered = localStorage.getItem(sessionKey);
    api
      .aiSessionsList()
      .then((list) => {
        setSessions(list);
        setActiveId(
          remembered && list.some((s) => s.id === remembered)
            ? remembered
            : null,
        );
      })
      .catch(() => {});
  }, [sessionKey]);

  useEffect(() => {
    if (skipLoadRef.current === activeId) {
      skipLoadRef.current = null;
      return;
    }
    void loadMessages(activeId);
  }, [loadMessages, activeId]);

  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  useEffect(() => {
    loadAssetMeta();
  }, [loadAssetMeta]);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && stickRef.current) el.scrollTop = el.scrollHeight;
  }, [messages, live?.text, live?.actions.length]);

  useEffect(() => {
    liveRef.current = live;
  }, [live]);

  // Closing the panel unmounts it: cancel a streaming turn instead of
  // orphaning it — its reply could never be watched again, and a reopened
  // panel would happily start a second concurrent turn on the same session.
  useEffect(
    () => () => {
      const l = liveRef.current;
      if (l) api.aiCancel(l.job).catch(() => {});
    },
    [],
  );

  useEffect(() => {
    if (menu === "none") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu("none");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [menu]);

  const switchSession = (id: string | null) => {
    setActiveId(id);
    setMenu("none");
    if (id) localStorage.setItem(sessionKey, id);
    else localStorage.removeItem(sessionKey);
  };

  const restoreSession = async () => {
    try {
      const id = await api.aiSessionRestore();
      await loadSessions();
      if (id) switchSession(id);
    } catch (e) {
      showToast(String(e));
    }
  };

  const deleteSession = async (id: string) => {
    // Deleting the conversation that is streaming: stop its run first
    const l = liveRef.current;
    if (l?.sessionId === id) api.aiCancel(l.job).catch(() => {});
    try {
      await api.aiSessionDelete(id);
      if (id === activeIdRef.current) switchSession(null);
      await loadSessions();
      // Undo over confirm — the same contract as file deletion
      showToast({
        text: t("aiSessionDeleted"),
        action: { label: t("undoAction"), fn: () => void restoreSession() },
      });
    } catch (e) {
      showToast(String(e));
    }
  };

  const updatePrefs = (next: Prefs) => {
    if (active) {
      setSessions((list) =>
        list.map((s) =>
          s.id === active.id
            ? {
                ...s,
                supply: next.supply ?? s.supply,
                model: next.model,
                effort: next.effort,
              }
            : s,
        ),
      );
      if (next.supply) {
        api
          .aiSessionSetPrefs(active.id, next.supply, next.model, next.effort)
          .catch(() => {});
      }
    } else {
      setDraft(next);
      // A draft's supply choice becomes the default for future conversations
      if (next.supply && next.supply !== config.supply) {
        const cfg = { ...config, supply: next.supply };
        setConfig(cfg);
        api.aiSetConfig(cfg).catch(() => {});
      }
    }
  };

  // The turn may finish while another session is on screen — refresh only
  // the transcript actually being shown
  const refreshAfterTurn = (sessionId: string) =>
    activeIdRef.current === sessionId
      ? loadMessages(sessionId)
      : Promise.resolve();

  const send = async () => {
    const text = input.trim();
    if (sendingRef.current || live || !text || !prefs.supply) return;
    // No default tier: a session always pins an explicit model — the custom
    // chip with an empty input must not create a model-less session
    if (!prefs.model.trim()) return;
    sendingRef.current = true;
    try {
      let sessionId = activeId;
      try {
        if (!sessionId) {
          const s = await api.aiSessionCreate(
            prefs.supply,
            prefs.model,
            prefs.effort,
          );
          setSessions((list) => [s, ...list]);
          sessionId = s.id;
          skipLoadRef.current = s.id;
          switchSession(s.id);
        }
      } catch (e) {
        showToast(String(e));
        return;
      }
      const job = crypto.randomUUID();
      setInput("");
      // Optimistic user turn; replaced by the authoritative list after the turn
      setMessages((m) => [
        ...m,
        {
          id: `local-${job}`,
          sessionId,
          role: "user",
          content: text,
          actions: [],
          createdAt: Math.floor(Date.now() / 1000),
        },
      ]);
      stickRef.current = true;
      setLive({ job, sessionId, instruction: text, actions: [], text: "" });
      try {
        await api.aiSend(
          {
            job,
            sessionId,
            text,
            currentAssetId: viewerAsset?.id ?? null,
          },
          (e) => {
            setLive((l) => {
              if (l?.job !== job) return l;
              if (e.type === "delta") return { ...l, text: l.text + e.text };
              const last = l.actions[l.actions.length - 1];
              return last === e.label
                ? l
                : { ...l, actions: [...l.actions, e.label] };
            });
          },
        );
        // Fetch BEFORE the finally clears the live block: both updates land
        // in one React batch, so the streamed reply never blinks out
        await refreshAfterTurn(sessionId);
        loadAssetMeta();
        void loadSessions();
      } catch (e) {
        showToast(localizeAiError(String(e)));
        // Restore the instruction only into the composer it came from
        if (activeIdRef.current === sessionId) setInput(text);
        await refreshAfterTurn(sessionId);
      } finally {
        setLive((l) => (l?.job === job ? null : l));
      }
    } finally {
      sendingRef.current = false;
    }
  };

  const stop = () => {
    const job = liveRef.current?.job;
    if (job) api.aiCancel(job).catch(() => {});
  };

  const rollback = async (toVer: number) => {
    if (!viewerAsset) return;
    try {
      await api.restoreVersion(viewerAsset.id, toVer);
      showToast(t("aiRolledBack", { n: toVer }));
    } catch (e) {
      showToast(String(e));
    }
  };

  const openDiff = (toVer: number) => {
    if (!viewerAsset) return;
    setMenu("none");
    useStore.getState().setModal({
      kind: "aiDiff",
      asset: viewerAsset,
      fromVer: toVer > 1 ? toVer - 1 : null,
      toVer,
    });
  };

  // Version cards under the assistant message that produced them (only for
  // the file currently open — cross-file writes stay visible as action rows)
  const runsByMessage = new Map<string, AiRun[]>();
  for (const r of runs) {
    if (r.messageId && r.ver != null && r.status === "ok") {
      const list = runsByMessage.get(r.messageId) ?? [];
      list.push(r);
      runsByMessage.set(r.messageId, list);
    }
  }
  const latestVer = versions.length ? versions[0].ver : 0;

  return (
    <aside className="ai-panel relative z-[5] flex shrink-0 flex-col border-l border-line bg-paper">
      <header className="flex h-10 shrink-0 items-center gap-1.5 border-b border-line px-2.5">
        <Sparkles className="h-4 w-4 shrink-0 text-primary" />
        <button
          onClick={() => setMenu(menu === "sessions" ? "none" : "sessions")}
          className="flex h-7 min-w-0 flex-1 items-center gap-1 rounded-ctl px-1.5 text-left transition hover:bg-side"
          title={t("aiSessions")}
        >
          <span className="min-w-0 flex-1 truncate text-xs font-bold">
            {active ? active.title || t("aiNewSession") : t("aiNewSession")}
          </span>
          <ChevronDown className="h-3 w-3 shrink-0 text-sub" />
        </button>
        <button
          onClick={() => switchSession(null)}
          title={t("aiNewSession")}
          className="grid h-6 w-6 shrink-0 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
        >
          <Plus className="h-3.5 w-3.5" />
        </button>
        {viewerAsset && versions.length > 0 && (
          <button
            onClick={() => setMenu(menu === "versions" ? "none" : "versions")}
            title={t("aiVersionsTitle")}
            className={`flex h-6 shrink-0 items-center gap-1 rounded-full border px-2 text-[10.5px] font-bold transition ${
              menu === "versions"
                ? "border-primary/40 bg-primary/10 text-primary"
                : "border-line bg-side text-sub2 hover:border-primary/40"
            }`}
          >
            <History className="h-3 w-3" />v{latestVer}
          </button>
        )}
        <button
          onClick={() => setMenu(menu === "prefs" ? "none" : "prefs")}
          title={t("aiSupply")}
          className={`grid h-6 w-6 shrink-0 place-items-center rounded-ctl transition ${
            menu === "prefs"
              ? "bg-primary/10 text-primary"
              : "text-sub hover:bg-side hover:text-ink"
          }`}
        >
          <Settings2 className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={toggleAi}
          title={`${t("aiPanelHide")} (⌘J)`}
          className="grid h-6 w-6 shrink-0 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </header>

      {menu !== "none" && (
        <button
          aria-label={t("aiCloseMenu")}
          onClick={() => setMenu("none")}
          className="fixed inset-0 z-10 cursor-default"
          tabIndex={-1}
        />
      )}
      {menu === "sessions" && (
        <SessionMenu
          t={t}
          lang={lang}
          sessions={sessions}
          activeId={activeId}
          onPick={switchSession}
          onDelete={(id) => void deleteSession(id)}
        />
      )}
      {menu === "prefs" && (
        <PrefsMenu
          t={t}
          options={options ?? []}
          prefs={prefs}
          onChange={updatePrefs}
        />
      )}
      {menu === "versions" && (
        <VersionsMenu
          t={t}
          lang={lang}
          versions={versions}
          runs={runs}
          latestVer={latestVer}
          onDiff={openDiff}
          onRollback={(v) => void rollback(v)}
        />
      )}

      {empty ? (
        <EmptyState t={t} />
      ) : (
        <div
          ref={scrollRef}
          onScroll={(e) => {
            const el = e.currentTarget;
            // Re-stick when the user returns to the bottom; let them read
            // upward without the stream yanking the scroll back down
            stickRef.current =
              el.scrollHeight - el.scrollTop - el.clientHeight < 48;
          }}
          className="min-h-0 flex-1 overflow-y-auto p-3"
        >
          <div className="flex flex-col gap-2.5">
            {messages.map((m) => (
              <MessageRow
                key={m.id}
                m={m}
                t={t}
                writes={runsByMessage.get(m.id) ?? []}
                latestVer={latestVer}
                onDiff={openDiff}
                onRollback={(v) => void rollback(v)}
              />
            ))}
            {live?.sessionId === activeId && <LiveBlock live={live} t={t} />}
          </div>
        </div>
      )}

      <footer className="shrink-0 border-t border-line p-3">
        {viewerAsset && (
          <div className="mb-1.5 flex items-center gap-1 text-[10.5px] text-sub">
            <span className="h-1 w-1 rounded-full bg-primary/60" />
            {t("aiAttached", { name: viewerAsset.fileName })}
          </div>
        )}
        <div className="flex items-end gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onCompositionEnd={() => {
              composeEndAt.current = Date.now();
            }}
            onKeyDown={(e) => {
              if (e.key !== "Enter" || e.shiftKey) return;
              // Mid-composition Enter (all engines)…
              if (e.nativeEvent.isComposing) return;
              // …and WKWebView's candidate-confirming Enter, which arrives
              // AFTER compositionend with isComposing already false — a bare
              // isComposing check would send half-typed pinyin
              if (Date.now() - composeEndAt.current < 100) return;
              e.preventDefault();
              void send();
            }}
            rows={2}
            disabled={!!live || empty}
            placeholder={
              viewerAsset
                ? t("aiPlaceholder", { name: viewerAsset.fileName })
                : t("aiPlaceholderGlobal")
            }
            className="max-h-32 min-h-[38px] flex-1 resize-none rounded-ctl border border-line bg-side px-2.5 py-2 text-xs outline-none placeholder:text-sub focus:border-primary disabled:opacity-50"
          />
          {live ? (
            <button
              onClick={stop}
              title={t("aiStop")}
              className="grid h-8 w-8 shrink-0 place-items-center rounded-ctl bg-danger/10 text-danger transition hover:bg-danger hover:text-white"
            >
              <Square className="h-3.5 w-3.5" />
            </button>
          ) : (
            <button
              onClick={() => void send()}
              disabled={
                empty || !prefs.supply || !prefs.model.trim() || !input.trim()
              }
              title={t("aiSend")}
              className="grid h-8 w-8 shrink-0 place-items-center rounded-ctl bg-primary text-white transition hover:bg-primary-light disabled:opacity-35"
            >
              <ArrowUp className="h-4 w-4" />
            </button>
          )}
        </div>
      </footer>
    </aside>
  );
}

function MenuShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="absolute top-10 right-2 left-2 z-20 max-h-[60%] overflow-y-auto rounded-xl border border-line bg-card p-1.5 shadow-xl">
      {children}
    </div>
  );
}

function SessionMenu({
  t,
  lang,
  sessions,
  activeId,
  onPick,
  onDelete,
}: {
  t: TFn;
  lang: string;
  sessions: AiSession[];
  activeId: string | null;
  onPick: (id: string | null) => void;
  onDelete: (id: string) => void;
}) {
  return (
    <MenuShell>
      <button
        onClick={() => onPick(null)}
        className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-xs font-bold text-primary transition hover:bg-primary/10"
      >
        <Plus className="h-3.5 w-3.5" />
        {t("aiNewSession")}
      </button>
      {sessions.map((s) => (
        <div
          key={s.id}
          className={`group flex items-center gap-2 rounded-lg px-2.5 py-1.5 transition hover:bg-side ${
            s.id === activeId ? "bg-primary/8" : ""
          }`}
        >
          <button
            onClick={() => onPick(s.id)}
            className="min-w-0 flex-1 text-left"
          >
            <div className="truncate text-xs">
              {s.title || t("aiNewSession")}
            </div>
            <div className="text-[10px] text-sub">
              {supplyLabel(s.supply)} · {timeAgo(s.updatedAt, lang)}
            </div>
          </button>
          {s.id === activeId && (
            <Check className="h-3 w-3 shrink-0 text-primary" />
          )}
          <button
            onClick={() => onDelete(s.id)}
            title={t("aiDeleteSession")}
            className="hidden h-5 w-5 shrink-0 place-items-center rounded text-sub transition group-hover:grid hover:bg-card hover:text-danger"
          >
            <Trash2 className="h-3 w-3" />
          </button>
        </div>
      ))}
    </MenuShell>
  );
}

/** Curated model choices per supply, chip-labeled. Ids verified against
 * provider docs 2026-07-04 — note OpenRouter spells Anthropic versions with
 * dots (claude-opus-4.8) while the Anthropic API uses dashes. First entry =
 * the initial pick for new conversations (there is deliberately no "default"
 * option — a session always pins an explicit model). The custom chip keeps
 * any future id usable. */
const MODEL_CHOICES: Record<AiSupply, { v: string; label: string }[]> = {
  claude: [
    { v: "sonnet", label: "Sonnet" },
    { v: "opus", label: "Opus" },
    { v: "haiku", label: "Haiku" },
    { v: "fable", label: "Fable" },
  ],
  codex: [
    { v: "gpt-5.5", label: "GPT-5.5" },
    { v: "gpt-5.4", label: "GPT-5.4" },
    { v: "gpt-5.4-mini", label: "5.4 Mini" },
  ],
  anthropic: [
    { v: "claude-sonnet-5", label: "Sonnet 5" },
    { v: "claude-opus-4-8", label: "Opus 4.8" },
    { v: "claude-haiku-4-5", label: "Haiku 4.5" },
    { v: "claude-fable-5", label: "Fable 5" },
  ],
  openai: [
    { v: "gpt-5.5", label: "GPT-5.5" },
    { v: "gpt-5.4", label: "GPT-5.4" },
    { v: "gpt-5.4-mini", label: "5.4 Mini" },
    { v: "gpt-5.2", label: "GPT-5.2" },
  ],
  openrouter: [
    { v: "anthropic/claude-sonnet-5", label: "Sonnet 5" },
    { v: "anthropic/claude-opus-4.8", label: "Opus 4.8" },
    { v: "openai/gpt-5.5", label: "GPT-5.5" },
    { v: "google/gemini-3.1-pro-preview", label: "Gemini 3.1 Pro" },
    { v: "google/gemini-3.5-flash", label: "Gemini 3.5 Flash" },
  ],
};

/** The effort levels each supply ACTUALLY accepts (verified 2026-07-04):
 * claude --effort and Anthropic output_config take low…max; codex
 * model_reasoning_effort takes minimal…xhigh; OpenAI reasoning_effort takes
 * the full none/minimal/low/medium/high/xhigh spectrum; OpenRouter's
 * reasoning.effort accepts everything and normalizes per upstream. First
 * entry = initial pick (claude's documented session default is high;
 * elsewhere medium). */
const EFFORT_CHOICES: Record<AiSupply, AiEffort[]> = {
  claude: ["high", "low", "medium", "xhigh", "max"],
  codex: ["medium", "minimal", "low", "high", "xhigh"],
  anthropic: ["medium", "low", "high", "xhigh", "max"],
  openai: ["medium", "none", "minimal", "low", "high", "xhigh"],
  openrouter: ["medium", "none", "minimal", "low", "high", "xhigh", "max"],
};

/** Initial model + effort when a conversation adopts this supply. */
function initialPrefsFor(supply: AiSupply): {
  model: string;
  effort: AiEffort;
} {
  return {
    model: MODEL_CHOICES[supply][0].v,
    effort: EFFORT_CHOICES[supply][0],
  };
}

/** Stable display order for effort chips, regardless of which one is the
 * initial pick. */
const EFFORT_ORDER: AiEffort[] = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
];

function PrefsMenu({
  t,
  options,
  prefs,
  onChange,
}: {
  t: TFn;
  options: SupplyOption[];
  prefs: Prefs;
  onChange: (p: Prefs) => void;
}) {
  const supply = prefs.supply;
  const choices = supply ? MODEL_CHOICES[supply] : [];
  const efforts = supply
    ? EFFORT_ORDER.filter((e) => EFFORT_CHOICES[supply].includes(e))
    : [];
  // The custom chip stays active while its value duplicates nothing curated
  const [custom, setCustom] = useState(
    () => prefs.model !== "" && !choices.some((c) => c.v === prefs.model),
  );

  const chip = (
    key: string,
    label: string,
    active: boolean,
    onClick: () => void,
    title?: string,
  ) => (
    <button
      key={key}
      onClick={onClick}
      title={title}
      className={`h-6 rounded-full border px-2.5 text-[10.5px] transition ${
        active
          ? "border-primary/40 bg-primary/10 font-bold text-primary"
          : "border-line bg-side text-sub2 hover:border-primary/40"
      }`}
    >
      {label}
    </button>
  );

  return (
    <MenuShell>
      <div className="px-2.5 pt-1.5 pb-1 text-[10.5px] font-bold text-sub">
        {t("aiSupply")}
      </div>
      {options.map((o) => (
        <button
          key={o.id}
          onClick={() => {
            setCustom(false);
            onChange({ ...prefs, supply: o.id, ...initialPrefsFor(o.id) });
          }}
          className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-xs transition hover:bg-side ${
            prefs.supply === o.id ? "font-bold text-primary" : ""
          }`}
        >
          <span className="min-w-0 flex-1 truncate text-left">
            {o.label}
            {o.local ? ` · ${t("aiSupplyLocal")}` : ""}
          </span>
          {prefs.supply === o.id && <Check className="h-3 w-3 shrink-0" />}
        </button>
      ))}
      <div className="px-2.5 pt-2 pb-1 text-[10.5px] font-bold text-sub">
        {t("aiModelLabel")}
      </div>
      <div className="flex flex-wrap gap-1 px-2.5 pb-1.5">
        {choices.map((c) =>
          chip(
            c.v,
            c.label,
            !custom && prefs.model === c.v,
            () => {
              setCustom(false);
              onChange({ ...prefs, model: c.v });
            },
            c.v,
          ),
        )}
        {chip("custom", t("aiModelCustom"), custom, () => setCustom(true))}
      </div>
      {custom && (
        <div className="px-2.5 pt-0.5 pb-1.5">
          <input
            autoFocus
            value={prefs.model}
            onChange={(e) => onChange({ ...prefs, model: e.target.value })}
            placeholder={choices[0]?.v ?? "model id"}
            className="h-7 w-full rounded-ctl border border-line bg-side px-2 text-[11px] outline-none focus:border-primary"
          />
        </div>
      )}
      <div className="px-2.5 pt-1 pb-1 text-[10.5px] font-bold text-sub">
        {t("aiEffortLabel")}
      </div>
      <div className="flex flex-wrap gap-1 px-2.5 pb-2">
        {efforts.map((e) =>
          chip(e, e, prefs.effort === e, () =>
            onChange({ ...prefs, effort: e }),
          ),
        )}
      </div>
    </MenuShell>
  );
}

function VersionsMenu({
  t,
  lang,
  versions,
  runs,
  latestVer,
  onDiff,
  onRollback,
}: {
  t: TFn;
  lang: string;
  versions: VersionInfo[];
  runs: AiRun[];
  latestVer: number;
  onDiff: (v: number) => void;
  onRollback: (v: number) => void;
}) {
  const instructionOf = (ver: number) =>
    runs.find((r) => r.ver === ver && r.status === "ok")?.instruction ?? "";
  return (
    <MenuShell>
      <div className="px-2.5 pt-1.5 pb-1 text-[10.5px] font-bold text-sub">
        {t("aiVersionsTitle")}
      </div>
      {versions.map((v) => (
        <div
          key={v.ver}
          className="group flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-[11px] transition hover:bg-side"
        >
          <span
            className={`shrink-0 font-bold ${v.ver === latestVer ? "text-primary" : "text-sub2"}`}
          >
            v{v.ver}
          </span>
          <span className="min-w-0 flex-1 truncate text-sub">
            {instructionOf(v.ver) || localizeVerLabel(v.label, t)}
          </span>
          <span className="shrink-0 text-[10px] text-sub">
            {timeAgo(v.createdAt, lang)}
          </span>
          <span className="hidden shrink-0 items-center gap-0.5 group-hover:flex">
            {v.ver > 1 && (
              <button
                onClick={() => onDiff(v.ver)}
                title={t("aiViewDiff")}
                className="grid h-5 w-5 place-items-center rounded text-sub transition hover:bg-card hover:text-ink"
              >
                <FileDiff className="h-3 w-3" />
              </button>
            )}
            {v.ver !== latestVer && (
              <button
                onClick={() => onRollback(v.ver)}
                title={t("aiRollbackTo", { n: v.ver })}
                className="grid h-5 w-5 place-items-center rounded text-sub transition hover:bg-card hover:text-ink"
              >
                <Undo2 className="h-3 w-3" />
              </button>
            )}
          </span>
        </div>
      ))}
    </MenuShell>
  );
}

function EmptyState({ t }: { t: TFn }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
      <Sparkles className="h-5 w-5 text-sub" />
      <div className="text-xs leading-relaxed text-sub">
        {t("aiEmptyTitle")}
      </div>
      <button
        onClick={() => useStore.getState().setModal({ kind: "settings" })}
        className="h-7 rounded-ctl bg-primary px-3 text-xs font-bold text-white transition hover:bg-primary-light"
      >
        {t("aiConfigure")}
      </button>
    </div>
  );
}

function MessageRow({
  m,
  t,
  writes,
  latestVer,
  onDiff,
  onRollback,
}: {
  m: AiMessage;
  t: TFn;
  writes: AiRun[];
  latestVer: number;
  onDiff: (v: number) => void;
  onRollback: (v: number) => void;
}) {
  if (m.role === "user") {
    return (
      <div className="ml-6 rounded-ctl bg-primary/8 px-2.5 py-1.5 text-xs leading-relaxed whitespace-pre-wrap select-text">
        {m.content}
      </div>
    );
  }
  return (
    <div className="mr-2">
      {m.actions.length > 0 && (
        <div className="mb-1 flex flex-col gap-0.5">
          {m.actions.map((a, i) => (
            <div
              key={`${a}${i}`}
              className="flex items-center gap-1.5 text-[10.5px] text-sub"
            >
              <span className="h-1 w-1 shrink-0 rounded-full bg-primary/60" />
              <span className="truncate font-mono">{a}</span>
            </div>
          ))}
        </div>
      )}
      <div className="ai-md text-xs leading-relaxed select-text">
        <Md text={m.content} />
      </div>
      {writes.map((w) => (
        <div
          key={w.id}
          className="mt-1.5 rounded-ctl border border-primary/25 bg-primary/8 px-2.5 py-2"
        >
          <div className="flex items-center gap-1.5 text-[11.5px] font-bold text-primary">
            <Sparkles className="h-3 w-3" />
            {t("aiGeneratedVer", { n: w.ver ?? 0 })}
          </div>
          <div className="mt-1.5 flex gap-1.5">
            <button
              onClick={() => w.ver != null && onDiff(w.ver)}
              className="h-6 rounded-ctl border border-primary/30 bg-card px-2 text-[10.5px] font-bold text-primary transition hover:bg-primary hover:text-white"
            >
              {t("aiViewDiff")}
            </button>
            {w.ver != null && w.ver > 1 && w.ver === latestVer && (
              <button
                onClick={() => {
                  if (w.ver != null) onRollback(w.ver - 1);
                }}
                className="h-6 rounded-ctl border border-line bg-card px-2 text-[10.5px] text-sub2 transition hover:border-primary/40"
              >
                {t("aiRollback")}
              </button>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

/** Markdown rendered once per distinct content — parsing is the hot cost, so
 * neither streaming deltas nor unrelated re-renders may re-run it for old
 * messages. Link clicks open in the browser (the webview must never navigate);
 * `javascript:` etc. are already stripped by react-markdown's urlTransform and
 * rejected backend-side. */
const MD_COMPONENTS: Components = {
  a: ({ href, children }) => (
    <a
      href={href}
      onClick={(e) => {
        e.preventDefault();
        if (href) api.openUrl(href).catch(() => {});
      }}
    >
      {children}
    </a>
  ),
};

function Md({ text }: { text: string }) {
  return useMemo(
    () => (
      <Markdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
        {text}
      </Markdown>
    ),
    [text],
  );
}

function LiveBlock({ live, t }: { live: LiveRun; t: TFn }) {
  // Live markdown, Claude Code-style: the streaming text renders through the
  // SAME pipeline as the persisted message (identical classes → the end-of-
  // turn swap is invisible). Unclosed constructs show as raw source until
  // their closing token streams in. useDeferredValue lets fast delta bursts
  // skip intermediate parses instead of reparsing per token.
  const text = useDeferredValue(live.text);
  return (
    <div className="mr-2">
      <div className="flex flex-col gap-0.5">
        {live.actions.slice(-6).map((a, i) => (
          <div
            key={`${a}${i}`}
            className="flex items-center gap-1.5 text-[10.5px] text-sub"
          >
            <span className="h-1 w-1 shrink-0 rounded-full bg-primary/60" />
            <span className="truncate font-mono">{a}</span>
          </div>
        ))}
      </div>
      <div className="mt-1 flex items-center gap-1.5 text-[11px] text-sub">
        <span className="h-3 w-3 shrink-0 animate-spin rounded-full border-[1.5px] border-primary/30 border-t-primary" />
        {t("aiRunning")}
      </div>
      {text && (
        <div className="ai-md mt-1 text-xs leading-relaxed select-text">
          <Md text={text} />
        </div>
      )}
    </div>
  );
}
