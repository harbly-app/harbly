import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { FolderOpen, FolderPlus, Loader2 } from "lucide-react";
import { api } from "../lib/api";
import { makeT } from "../lib/i18n";
import { useStore } from "../lib/store";
import type { ScanProgress } from "../lib/types";

export default function Onboarding() {
  const [step, setStep] = useState<0 | 1>(0);
  const [defaultPath, setDefaultPath] = useState("~/Harbly");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [scanDone, setScanDone] = useState(false);
  const enterMain = useStore((s) => s.enterMain);
  const t = makeT(useStore((s) => s.lang));
  const unlisten = useRef<(() => void) | null>(null);

  useEffect(() => {
    api
      .defaultLibraryPath()
      .then(setDefaultPath)
      .catch(() => {});
    return () => unlisten.current?.();
  }, []);

  const start = async (path: string) => {
    setBusy(true);
    setErr(null);
    try {
      await api.libraryInit(path);
      setStep(1);
      unlisten.current = await listen<ScanProgress>("scan-progress", (e) =>
        setProgress(e.payload),
      );
      api
        .scanLibrary()
        .then(() => setScanDone(true))
        .catch(() => setScanDone(true));
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const createNew = () => start(defaultPath);

  const changeLocation = async () => {
    const dir = await api.pickFolder();
    if (dir) setDefaultPath(`${dir}/Harbly`);
  };

  const adoptExisting = async () => {
    const dir = await api.pickFolder();
    if (dir) await start(dir);
  };

  // Split "create at {path}…" per-language word order; the path segment uses a monospace font
  const [descPre, descPost] = t("obNewLibDesc").split("{path}");

  return (
    <div
      className="flex h-screen flex-col items-center justify-center bg-paper"
      data-tauri-drag-region
    >
      <div className="w-[520px]">
        <div className="mb-8 flex items-center gap-3">
          <div className="grid h-10 w-10 place-items-center rounded-[13px] bg-primary text-base font-extrabold text-white">
            {"</>"}
          </div>
          <div>
            <div className="text-xl font-extrabold">{t("welcome")}</div>
            <div className="mt-0.5 text-xs text-sub">
              {step === 0 ? t("obStep0") : t("obStep1")}
            </div>
          </div>
        </div>

        {step === 0 && (
          <div className="space-y-3">
            <button
              onClick={createNew}
              disabled={busy}
              className="group w-full rounded-card border border-line bg-card p-4 text-left transition hover:border-primary/50 hover:shadow-sm disabled:opacity-50"
            >
              <div className="flex items-center gap-3">
                <FolderPlus className="h-5 w-5 text-primary" />
                <div className="flex-1">
                  <div className="font-bold">{t("obNewLib")}</div>
                  <div className="mt-0.5 text-xs text-sub">
                    {descPre}
                    <span className="font-mono">{defaultPath}</span>
                    {descPost}
                  </div>
                </div>
                <span
                  onClick={(e) => {
                    e.stopPropagation();
                    void changeLocation();
                  }}
                  className="shrink-0 text-xs text-primary hover:underline"
                >
                  {t("obChangeLocation")}
                </span>
              </div>
            </button>

            <button
              onClick={adoptExisting}
              disabled={busy}
              className="w-full rounded-card border border-line bg-card p-4 text-left transition hover:border-primary/50 hover:shadow-sm disabled:opacity-50"
            >
              <div className="flex items-center gap-3">
                <FolderOpen className="h-5 w-5 text-primary" />
                <div>
                  <div className="font-bold">{t("obAdopt")}</div>
                  <div className="mt-0.5 text-xs text-sub">
                    {t("obAdoptDesc")}
                  </div>
                </div>
              </div>
            </button>

            {err && <div className="text-xs text-danger">{err}</div>}

            <p className="pt-2 text-[11px] leading-relaxed text-sub">
              {t("obFootnote")}
            </p>
          </div>
        )}

        {step === 1 && (
          <div className="rounded-card border border-line bg-card p-5">
            <div className="flex items-center gap-2 text-sm font-bold">
              {!scanDone && (
                <Loader2 className="h-4 w-4 animate-spin text-primary" />
              )}
              {scanDone ? t("scanDone") : t("scanningTitle")}
            </div>
            <div className="mt-3 space-y-1.5 text-xs text-sub2">
              <div className="flex justify-between">
                <span>{t("foundFiles")}</span>
                <span className="font-semibold">
                  {t("countUnit", { n: progress?.found ?? 0 })}
                </span>
              </div>
              <div className="flex justify-between">
                <span>{t("indexedFiles")}</span>
                <span className="font-semibold">
                  {t("countUnit", { n: progress?.indexed ?? 0 })}
                </span>
              </div>
              <div className="flex justify-between">
                <span>{t("thumbsRow")}</span>
                <span>{scanDone ? t("thumbsBg") : t("thumbsWaiting")}</span>
              </div>
            </div>
            <button
              onClick={enterMain}
              className="mt-5 w-full rounded-ctl bg-primary py-2.5 text-sm font-bold text-white transition hover:bg-primary-light"
            >
              {t("enterApp")}
            </button>
            {!scanDone && (
              <div className="mt-2 text-center text-[11px] text-sub">
                {t("scanContinuesBg")}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
