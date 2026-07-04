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
    api.defaultLibraryPath().then(setDefaultPath).catch(() => {});
    return () => unlisten.current?.();
  }, []);

  const start = async (path: string) => {
    setBusy(true);
    setErr(null);
    try {
      await api.libraryInit(path);
      setStep(1);
      unlisten.current = await listen<ScanProgress>("scan-progress", (e) =>
        setProgress(e.payload)
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
    if (dir) start(dir);
  };

  // Split "create at {path}…" per-language word order; the path segment uses a monospace font
  const [descPre, descPost] = t("obNewLibDesc").split("{path}");

  return (
    <div className="h-screen flex flex-col items-center justify-center bg-paper" data-tauri-drag-region>
      <div className="w-[520px]">
        <div className="flex items-center gap-3 mb-8">
          <div className="w-10 h-10 rounded-[13px] bg-primary text-white grid place-items-center text-base font-extrabold">
            {"</>"}
          </div>
          <div>
            <div className="text-xl font-extrabold">{t("welcome")}</div>
            <div className="text-xs text-sub mt-0.5">{step === 0 ? t("obStep0") : t("obStep1")}</div>
          </div>
        </div>

        {step === 0 && (
          <div className="space-y-3">
            <button
              onClick={createNew}
              disabled={busy}
              className="w-full text-left bg-white border border-line rounded-card p-4 hover:border-primary/50 hover:shadow-sm transition group disabled:opacity-50"
            >
              <div className="flex items-center gap-3">
                <FolderPlus className="w-5 h-5 text-primary" />
                <div className="flex-1">
                  <div className="font-bold">{t("obNewLib")}</div>
                  <div className="text-xs text-sub mt-0.5">
                    {descPre}
                    <span className="font-mono">{defaultPath}</span>
                    {descPost}
                  </div>
                </div>
                <span
                  onClick={(e) => {
                    e.stopPropagation();
                    changeLocation();
                  }}
                  className="text-xs text-primary hover:underline shrink-0"
                >
                  {t("obChangeLocation")}
                </span>
              </div>
            </button>

            <button
              onClick={adoptExisting}
              disabled={busy}
              className="w-full text-left bg-white border border-line rounded-card p-4 hover:border-primary/50 hover:shadow-sm transition disabled:opacity-50"
            >
              <div className="flex items-center gap-3">
                <FolderOpen className="w-5 h-5 text-primary" />
                <div>
                  <div className="font-bold">{t("obAdopt")}</div>
                  <div className="text-xs text-sub mt-0.5">{t("obAdoptDesc")}</div>
                </div>
              </div>
            </button>

            {err && <div className="text-xs text-danger">{err}</div>}

            <p className="text-[11px] leading-relaxed text-sub pt-2">{t("obFootnote")}</p>
          </div>
        )}

        {step === 1 && (
          <div className="bg-white border border-line rounded-card p-5">
            <div className="flex items-center gap-2 text-sm font-bold">
              {!scanDone && <Loader2 className="w-4 h-4 animate-spin text-primary" />}
              {scanDone ? t("scanDone") : t("scanningTitle")}
            </div>
            <div className="mt-3 space-y-1.5 text-xs text-sub2">
              <div className="flex justify-between">
                <span>{t("foundFiles")}</span>
                <span className="font-semibold">{t("countUnit", { n: progress?.found ?? 0 })}</span>
              </div>
              <div className="flex justify-between">
                <span>{t("indexedFiles")}</span>
                <span className="font-semibold">{t("countUnit", { n: progress?.indexed ?? 0 })}</span>
              </div>
              <div className="flex justify-between">
                <span>{t("thumbsRow")}</span>
                <span>{scanDone ? t("thumbsBg") : t("thumbsWaiting")}</span>
              </div>
            </div>
            <button
              onClick={enterMain}
              className="mt-5 w-full bg-primary text-white rounded-ctl py-2.5 text-sm font-bold hover:bg-primary-light transition"
            >
              {t("enterApp")}
            </button>
            {!scanDone && (
              <div className="text-center text-[11px] text-sub mt-2">{t("scanContinuesBg")}</div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
