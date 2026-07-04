import {
  ArrowUp,
  ChevronDown,
  CircleAlert,
  FileDiff,
  Sparkles,
  Square,
  Undo2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, timeAgo } from "../lib/api";
import { localizeAiError, localizeVerLabel, makeT } from "../lib/i18n";
import type { TFn } from "../lib/i18n";
import { useStore } from "../lib/store";
import type {
  AgentInfo,
  AiConfig,
  AiRun,
  AiSupply,
  AssetMeta,
  VersionInfo,
} from "../lib/types";

/** One selectable supply in the header dropdown */
interface SupplyOption {
  id: AiSupply;
  label: string;
  local: boolean;
}

interface LiveRun {
  job: string;
  instruction: string;
  actions: string[];
  text: string;
}

/** Timeline row: a version (optionally produced by an AI run) or a standalone
 * run (review report / failure). Sorted oldest→newest, chat-like. */
type TimelineItem =
  | { t: "ver"; time: number; ver: VersionInfo; run?: AiRun }
  | { t: "run"; time: number; run: AiRun };

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

function mergeTimeline(versions: VersionInfo[], runs: AiRun[]): TimelineItem[] {
  const byVer = new Map<number, AiRun>();
  const standalone: AiRun[] = [];
  for (const r of runs) {
    if (r.kind === "revise" && r.status === "ok" && r.ver != null)
      byVer.set(r.ver, r);
    else standalone.push(r);
  }
  const items: TimelineItem[] = versions.map((v) => ({
    t: "ver",
    time: v.createdAt,
    ver: v,
    run: byVer.get(v.ver),
  }));
  for (const r of standalone)
    items.push({ t: "run", time: r.createdAt, run: r });
  return items.sort((a, b) => a.time - b.time);
}

export default function AiPanel({ asset }: { asset: AssetMeta }) {
  const t = makeT(useStore((s) => s.lang));
  const toggleAi = useStore((s) => s.toggleAi);
  const showToast = useStore((s) => s.showToast);

  const [options, setOptions] = useState<SupplyOption[] | null>(null);
  const [config, setConfig] = useState<AiConfig>({});
  const [supply, setSupply] = useState<AiSupply | null>(null);
  const [versions, setVersions] = useState<VersionInfo[]>([]);
  const [runs, setRuns] = useState<AiRun[]>([]);
  const [input, setInput] = useState("");
  const [live, setLive] = useState<LiveRun | null>(null);
  const [result, setResult] = useState<AiRun | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const liveRef = useRef<LiveRun | null>(null);

  // Supplies: detected agents + configured keys, ordered local-first.
  // Re-probed each time the panel mounts (cheap; keeps state honest after the
  // user installs a CLI or saves a key in settings).
  const loadSupplies = useCallback(() => {
    return Promise.all([
      api.aiDetectAgents().catch((): AgentInfo[] => []),
      api.aiKeyStatus().catch((): Record<string, boolean> => ({})),
      api.aiGetConfig().catch((): AiConfig => ({})),
    ]).then(([agents, keys, cfg]) => {
      const opts = buildOptions(agents, keys);
      setOptions(opts);
      setConfig(cfg);
      setSupply((cur) => {
        const want = cur ?? cfg.supply ?? null;
        return want != null && opts.some((o) => o.id === want)
          ? want
          : (opts[0]?.id ?? null);
      });
    });
  }, []);

  const loadTimeline = useCallback(() => {
    return Promise.all([
      api.listVersions(asset.id).catch(() => [] as VersionInfo[]),
      api.aiRunsList(asset.id).catch(() => [] as AiRun[]),
    ]).then(([vs, rs]) => {
      setVersions(vs);
      setRuns(rs);
    });
  }, [asset.id]);

  const aiConfigEpoch = useStore((s) => s.aiConfigEpoch);
  useEffect(() => {
    void loadSupplies();
  }, [loadSupplies, aiConfigEpoch]);

  // currentHash changes on every content write (AI, rollback, external edit) —
  // exactly when the version chain may have grown.
  useEffect(() => {
    void loadTimeline();
  }, [loadTimeline, asset.currentHash]);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [versions, runs, live?.text, live?.actions.length, result]);

  // A run outlives the panel (user may close it or switch files): cancel is
  // explicit-only, but drop the local live state on unmount.
  useEffect(() => {
    liveRef.current = live;
  }, [live]);

  const pickSupply = (id: AiSupply) => {
    setSupply(id);
    const next = { ...config, supply: id };
    setConfig(next);
    api.aiSetConfig(next).catch(() => {});
  };

  // One send path for everything: change requests, questions, reviews. The
  // model routes intent; the backend classifies the outcome by diff.
  const send = async () => {
    if (live || !supply) return;
    const instruction = input.trim();
    if (!instruction) return;
    const job = crypto.randomUUID();
    setResult(null);
    setInput("");
    setLive({ job, instruction, actions: [], text: "" });
    try {
      const record = await api.aiRun(
        {
          job,
          id: asset.id,
          instruction,
          supply,
          model:
            config.models?.[supply as "anthropic" | "openai" | "openrouter"],
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
      setResult(record);
      await loadTimeline();
    } catch (e) {
      showToast(localizeAiError(String(e)));
      setInput(instruction);
    } finally {
      setLive((l) => (l?.job === job ? null : l));
    }
  };

  const stop = () => {
    const job = liveRef.current?.job;
    if (job) api.aiCancel(job).catch(() => {});
  };

  const rollback = async (toVer: number) => {
    try {
      await api.restoreVersion(asset.id, toVer);
      setResult(null);
      showToast(t("aiRolledBack", { n: toVer }));
    } catch (e) {
      showToast(String(e));
    }
  };

  const openDiff = (toVer: number) => {
    useStore.getState().setModal({
      kind: "aiDiff",
      asset,
      fromVer: toVer > 1 ? toVer - 1 : null,
      toVer,
    });
  };

  const latestVer = versions.length ? versions[0].ver : 0;
  const timeline = mergeTimeline(versions, runs);
  const empty = options !== null && options.length === 0;

  return (
    <aside className="ai-panel relative z-[5] flex shrink-0 flex-col border-l border-line bg-paper">
      <header className="flex h-10 shrink-0 items-center gap-2 border-b border-line px-3">
        <Sparkles className="h-4 w-4 text-primary" />
        <span className="text-[13px] font-extrabold">AI</span>
        <div className="flex-1" />
        {options && options.length > 0 && (
          <div className="relative flex h-6 items-center rounded-full border border-line bg-side pr-1.5 pl-2.5 text-[11px] text-sub2">
            <select
              value={supply ?? undefined}
              onChange={(e) => pickSupply(e.target.value as AiSupply)}
              className="max-w-[150px] cursor-pointer appearance-none truncate bg-transparent pr-4 outline-none"
              aria-label={t("aiSupply")}
            >
              {options.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label}
                  {o.local ? ` · ${t("aiSupplyLocal")}` : ""}
                </option>
              ))}
            </select>
            <ChevronDown className="pointer-events-none absolute right-1.5 h-3 w-3" />
          </div>
        )}
        <button
          onClick={toggleAi}
          title={`${t("aiPanelHide")} (⌘J)`}
          className="grid h-6 w-6 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </header>

      {empty ? (
        <EmptyState t={t} />
      ) : (
        <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto p-3">
          <div className="flex flex-col gap-2">
            {timeline.map((item) =>
              item.t === "ver" ? (
                <VersionRow
                  key={`v${item.ver.ver}`}
                  item={item}
                  t={t}
                  isLatest={item.ver.ver === latestVer}
                  onDiff={() => openDiff(item.ver.ver)}
                  onRollback={() => void rollback(item.ver.ver)}
                />
              ) : (
                <RunRow key={item.run.id} run={item.run} t={t} />
              ),
            )}
            {live && <LiveBlock live={live} t={t} />}
            {result && (
              <ResultCard
                result={result}
                t={t}
                onDiff={() => result.ver != null && openDiff(result.ver)}
                onRollback={() =>
                  result.ver != null &&
                  result.ver > 1 &&
                  void rollback(result.ver - 1)
                }
              />
            )}
          </div>
        </div>
      )}

      <footer className="shrink-0 border-t border-line p-3">
        <div className="flex items-end gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (
                e.key === "Enter" &&
                !e.shiftKey &&
                !e.nativeEvent.isComposing
              ) {
                e.preventDefault();
                void send();
              }
            }}
            rows={2}
            disabled={!!live || empty}
            placeholder={t("aiPlaceholder", { name: asset.fileName })}
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
              disabled={empty || !supply || !input.trim()}
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

function VersionRow({
  item,
  t,
  isLatest,
  onDiff,
  onRollback,
}: {
  item: { ver: VersionInfo; run?: AiRun };
  t: TFn;
  isLatest: boolean;
  onDiff: () => void;
  onRollback: () => void;
}) {
  const { ver, run } = item;
  return (
    <div className="group">
      <div className="flex items-center gap-2 text-[11px] text-sub">
        <span
          className={`h-[5px] w-[5px] shrink-0 rounded-full ${isLatest ? "bg-primary" : "bg-line-strong"}`}
        />
        <span
          className={`shrink-0 font-bold ${isLatest ? "text-primary" : ""}`}
        >
          v{ver.ver}
        </span>
        <span className="truncate">{localizeVerLabel(ver.label, t)}</span>
        <span className="ml-auto shrink-0">{timeAgo(ver.createdAt)}</span>
        <span className="hidden shrink-0 items-center gap-1 group-hover:flex">
          {ver.ver > 1 && (
            <button
              onClick={onDiff}
              title={t("aiViewDiff")}
              className="grid h-5 w-5 place-items-center rounded text-sub transition hover:bg-side hover:text-ink"
            >
              <FileDiff className="h-3 w-3" />
            </button>
          )}
          {!isLatest && (
            <button
              onClick={onRollback}
              title={t("aiRollbackTo", { n: ver.ver })}
              className="grid h-5 w-5 place-items-center rounded text-sub transition hover:bg-side hover:text-ink"
            >
              <Undo2 className="h-3 w-3" />
            </button>
          )}
        </span>
      </div>
      {run && (
        <div className="mt-1 ml-[13px] rounded-ctl bg-side px-2.5 py-1.5 text-xs leading-relaxed text-sub2">
          {run.instruction}
        </div>
      )}
    </div>
  );
}

function RunRow({ run, t }: { run: AiRun; t: TFn }) {
  // Any successful textual reply renders as a card: answers, reviews, and
  // legacy "review" records all share this shape.
  if (run.status === "ok" && run.report) {
    return (
      <div className="rounded-ctl border border-line bg-card px-3 py-2.5">
        <div className="mb-1.5 flex items-start gap-1.5 text-[11px] font-bold text-primary">
          <Sparkles className="mt-0.5 h-3 w-3 shrink-0" />
          <span className="line-clamp-2 min-w-0 flex-1">
            {run.instruction.trim() || t("aiReportTitle")}
          </span>
          <span className="shrink-0 font-normal text-sub">
            {timeAgo(run.createdAt)}
          </span>
        </div>
        <div className="max-h-64 overflow-y-auto text-xs leading-relaxed whitespace-pre-wrap text-sub2 select-text">
          {run.report}
        </div>
      </div>
    );
  }
  // Failed / cancelled / no-change runs: one muted line each
  const text =
    run.status === "cancelled"
      ? t("aiCancelledLine")
      : run.status === "error"
        ? localizeAiError(run.error ?? "")
        : t("aiNoChange");
  return (
    <div className="flex items-start gap-2 text-[11px] text-sub">
      <CircleAlert
        className={`mt-0.5 h-3 w-3 shrink-0 ${run.status === "error" ? "text-danger" : ""}`}
      />
      <span className="min-w-0 flex-1">
        {run.instruction && (
          <span className="mr-1.5 text-sub2">「{run.instruction}」</span>
        )}
        {text}
      </span>
      <span className="shrink-0">{timeAgo(run.createdAt)}</span>
    </div>
  );
}

function LiveBlock({ live, t }: { live: LiveRun; t: TFn }) {
  // Show only the streaming tail: a full HTML rewrite would flood the panel
  const tail = live.text.length > 600 ? `…${live.text.slice(-600)}` : live.text;
  return (
    <div>
      {live.instruction && (
        <div className="rounded-ctl bg-primary/8 px-2.5 py-1.5 text-xs leading-relaxed text-ink">
          {live.instruction}
        </div>
      )}
      <div className="mt-1.5 flex flex-col gap-1">
        {live.actions.slice(-5).map((a, i) => (
          <div
            key={`${a}${i}`}
            className="flex items-center gap-1.5 text-[11px] text-sub"
          >
            <span className="h-1 w-1 shrink-0 rounded-full bg-primary/60" />
            <span className="truncate font-mono">{a}</span>
          </div>
        ))}
      </div>
      <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-sub">
        <span className="h-3 w-3 shrink-0 animate-spin rounded-full border-[1.5px] border-primary/30 border-t-primary" />
        {t("aiRunning")}
        {live.text && (
          <span className="text-sub/70">
            {t("aiCharsStreamed", { n: live.text.length })}
          </span>
        )}
      </div>
      {tail && (
        <pre className="mt-1.5 max-h-28 overflow-hidden rounded-ctl bg-side px-2.5 py-2 font-mono text-[10.5px] leading-relaxed break-all whitespace-pre-wrap text-sub">
          {tail}
        </pre>
      )}
    </div>
  );
}

function ResultCard({
  result,
  t,
  onDiff,
  onRollback,
}: {
  result: AiRun;
  t: TFn;
  onDiff: () => void;
  onRollback: () => void;
}) {
  if (result.status === "ok" && result.ver != null) {
    return (
      <div className="rounded-ctl border border-primary/25 bg-primary/8 px-3 py-2.5">
        <div className="flex items-center gap-1.5 text-xs font-bold text-primary">
          <Sparkles className="h-3.5 w-3.5" />
          {t("aiGeneratedVer", { n: result.ver })}
        </div>
        <div className="mt-0.5 text-[11px] text-sub2">
          {t("aiPreviewSwitched")}
        </div>
        <div className="mt-2 flex gap-1.5">
          <button
            onClick={onDiff}
            className="h-6 rounded-ctl border border-primary/30 bg-card px-2.5 text-[11px] font-bold text-primary transition hover:bg-primary hover:text-white"
          >
            {t("aiViewDiff")}
          </button>
          {result.ver > 1 && (
            <button
              onClick={onRollback}
              className="h-6 rounded-ctl border border-line bg-card px-2.5 text-[11px] text-sub2 transition hover:border-primary/40"
            >
              {t("aiRollback")}
            </button>
          )}
        </div>
      </div>
    );
  }
  if (result.status === "ok") {
    // Textual replies show as a timeline card after reload; only a run that
    // produced neither a version nor a reply needs an inline note.
    return result.report ? null : (
      <div className="text-center text-[11px] text-sub">{t("aiNoChange")}</div>
    );
  }
  if (result.status === "cancelled") {
    return (
      <div className="text-center text-[11px] text-sub">
        {t("aiCancelledLine")}
      </div>
    );
  }
  return (
    <div className="flex items-start gap-2 rounded-ctl border border-danger/25 bg-danger/5 px-3 py-2 text-[11.5px] leading-relaxed text-danger">
      <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <span className="select-text">{localizeAiError(result.error ?? "")}</span>
    </div>
  );
}
