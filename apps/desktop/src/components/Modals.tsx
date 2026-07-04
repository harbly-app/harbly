import { Folder, FolderOpen, Hash, Images, Inbox as InboxIcon, Monitor, Moon, RefreshCw, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { LANGS, makeT } from "../lib/i18n";
import { useStore } from "../lib/store";
import type { ThemePref } from "../lib/theme";
import type { TreeNode } from "../lib/types";

export default function Modals() {
  const modal = useStore((s) => s.modal);
  const setModal = useStore((s) => s.setModal);

  useEffect(() => {
    if (!modal) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setModal(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [modal, setModal]);

  if (!modal) return null;

  return (
    <div
      className="fixed inset-0 z-50 bg-black/30 grid place-items-center"
      onMouseDown={() => setModal(null)}
    >
      <div
        className="w-[420px] bg-card rounded-card shadow-2xl border border-line p-5"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {modal.kind === "move" && <Move />}
        {modal.kind === "newFolder" && <NewFolder />}
        {modal.kind === "tags" && <Tags />}
        {modal.kind === "confirmDeleteFolder" && <ConfirmDeleteFolder />}
        {modal.kind === "settings" && <Settings />}
      </div>
    </div>
  );
}

function Title({ children }: { children: React.ReactNode }) {
  return <div className="text-[14px] font-extrabold mb-3">{children}</div>;
}

function Buttons(props: {
  okLabel: string;
  danger?: boolean;
  onOk: () => void;
  okDisabled?: boolean;
  autoFocusOk?: boolean;
}) {
  const setModal = useStore((s) => s.setModal);
  const t = makeT(useStore((s) => s.lang));
  return (
    <div className="mt-4 flex justify-end gap-2">
      <button
        onClick={() => setModal(null)}
        className="h-8 px-3.5 rounded-ctl text-xs font-bold bg-side border border-line hover:bg-line/60 transition"
      >
        {t("cancel")}
      </button>
      <button
        autoFocus={props.autoFocusOk}
        onClick={props.onOk}
        disabled={props.okDisabled}
        className={`h-8 px-3.5 rounded-ctl text-xs font-bold text-white transition disabled:opacity-40 ${
          props.danger ? "bg-danger hover:opacity-90" : "bg-primary hover:bg-primary-light"
        }`}
      >
        {props.okLabel}
      </button>
    </div>
  );
}

function flatten(tree: TreeNode | null, rootLabel: string): { rel: string; label: string; depth: number }[] {
  const out: { rel: string; label: string; depth: number }[] = [{ rel: "", label: rootLabel, depth: 0 }];
  const walk = (n: TreeNode, d: number) => {
    out.push({ rel: n.rel, label: n.name, depth: d });
    n.children.forEach((c) => walk(c, d + 1));
  };
  tree?.children.forEach((c) => walk(c, 1));
  return out;
}

function Move() {
  const modal = useStore((s) => s.modal);
  const tree = useStore((s) => s.tree);
  const doMove = useStore((s) => s.doMove);
  const t = makeT(useStore((s) => s.lang));
  const m = modal?.kind === "move" ? modal : null;
  if (!m) return null;

  const targets = flatten(tree, t("libraryRoot")).filter(
    (x) => m.fromFolder == null || x.rel !== m.fromFolder
  );

  return (
    <>
      <Title>{t("moveTitle", { name: m.label })}</Title>
      <div className="max-h-[300px] overflow-y-auto -mx-1.5 px-1.5 space-y-0.5">
        {targets.map((x) => (
          <button
            key={x.rel}
            onClick={() => doMove(m.ids, x.rel)}
            className="w-full flex items-center gap-2 px-2.5 py-2 rounded-ctl text-[12.5px] hover:bg-primary/10 hover:text-primary transition"
            style={{ paddingLeft: 10 + x.depth * 16 }}
          >
            {x.rel === "" ? <InboxIcon className="w-3.5 h-3.5 opacity-0" /> : <Folder className="w-3.5 h-3.5 text-sub" />}
            <span className="truncate">{x.label}</span>
          </button>
        ))}
      </div>
      <div className="text-[10.5px] text-sub mt-2">{t("moveHint")}</div>
    </>
  );
}

function NewFolder() {
  const modal = useStore((s) => s.modal);
  const doCreateFolder = useStore((s) => s.doCreateFolder);
  const t = makeT(useStore((s) => s.lang));
  const [name, setName] = useState("");
  const parent = modal?.kind === "newFolder" ? modal.parent : "";

  const ok = () => name.trim() && doCreateFolder(parent, name.trim());

  return (
    <>
      <Title>
        {t("newFolder")}
        {parent ? t("newFolderIn", { name: parent }) : ""}
      </Title>
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && ok()}
        placeholder={t("folderNamePlaceholder")}
        className="w-full h-9 px-3 rounded-ctl border border-line bg-side outline-none focus:border-primary text-[13px]"
      />
      <Buttons okLabel={t("create")} onOk={ok} okDisabled={!name.trim()} />
    </>
  );
}

function ConfirmDeleteFolder() {
  const modal = useStore((s) => s.modal);
  const setModal = useStore((s) => s.setModal);
  const doDeleteFolder = useStore((s) => s.doDeleteFolder);
  const t = makeT(useStore((s) => s.lang));
  const m = modal?.kind === "confirmDeleteFolder" ? modal : null;
  if (!m) return null;

  const ok = () => {
    setModal(null);
    doDeleteFolder(m.rel);
  };

  return (
    <>
      <Title>{t("confirmDelFolderTitle")}</Title>
      <div className="text-[12.5px] text-sub2 leading-relaxed">{t("confirmDelFolderBody", { name: m.label })}</div>
      {/* autoFocus lands Enter on the destructive button (the requested fast-confirm); Esc closes via the global handler */}
      <Buttons okLabel={t("confirmDelete")} danger autoFocusOk onOk={ok} />
    </>
  );
}

function Tags() {
  const modal = useStore((s) => s.modal);
  const allTags = useStore((s) => s.tags);
  const setModal = useStore((s) => s.setModal);
  const showToast = useStore((s) => s.showToast);
  const t = makeT(useStore((s) => s.lang));
  const asset = modal?.kind === "tags" ? modal.asset : null;
  const [selected, setSelected] = useState<Set<string>>(new Set(asset?.tags ?? []));
  const [input, setInput] = useState("");

  if (!asset) return null;

  const candidates = Array.from(new Set([...allTags.map((x) => x.name), ...selected]));

  const toggle = (x: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(x)) next.delete(x);
      else next.add(x);
      return next;
    });

  const addInput = () => {
    const x = input.trim().replace(/^#/, "");
    if (!x) return;
    setSelected((prev) => new Set(prev).add(x));
    setInput("");
  };

  const ok = async () => {
    // Include un-committed input text in the save too, so it is not silently dropped
    const final = new Set(selected);
    const pending = input.trim().replace(/^#/, "");
    if (pending) final.add(pending);
    try {
      await api.setTags(asset.id, [...final]);
      setModal(null);
      showToast(final.size > 0 ? t("savedTags", { n: final.size }) : t("clearedTags"));
    } catch (e) {
      showToast(String(e));
    }
  };

  return (
    <>
      <Title>{t("tagsTitle", { name: asset.fileName })}</Title>
      <input
        autoFocus
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key !== "Enter") return;
          // Has content = add as a candidate; empty input = save directly
          if (input.trim()) addInput();
          else ok();
        }}
        placeholder={t("tagsPlaceholder")}
        className="w-full h-9 px-3 rounded-ctl border border-line bg-side outline-none focus:border-primary text-[13px]"
      />
      <div className="mt-3 flex flex-wrap gap-1.5 max-h-[200px] overflow-y-auto">
        {candidates.length === 0 && <span className="text-xs text-sub">{t("noTagsYet")}</span>}
        {candidates.map((x) => {
          const on = selected.has(x);
          return (
            <button
              key={x}
              onClick={() => toggle(x)}
              className={`flex items-center gap-1 px-2.5 py-1 rounded-full text-[11.5px] font-bold border transition ${
                on ? "bg-primary text-white border-primary" : "bg-card text-sub2 border-line hover:border-primary/50"
              }`}
            >
              <Hash className="w-3 h-3" />
              {x}
            </button>
          );
        })}
      </div>
      <Buttons okLabel={t("save")} onOk={ok} />
    </>
  );
}

function Settings() {
  const root = useStore((s) => s.root);
  const showToast = useStore((s) => s.showToast);
  const setModal = useStore((s) => s.setModal);
  const enterMain = useStore((s) => s.enterMain);
  const lang = useStore((s) => s.lang);
  const setLang = useStore((s) => s.setLang);
  const theme = useStore((s) => s.theme);
  const setTheme = useStore((s) => s.setTheme);
  const t = makeT(lang);
  const [busy, setBusy] = useState(false);

  // macOS Appearance order: Light · Dark · Auto (follow system)
  const themeOptions: { value: ThemePref; icon: typeof Sun; label: string }[] = [
    { value: "light", icon: Sun, label: t("themeLight") },
    { value: "dark", icon: Moon, label: t("themeDark") },
    { value: "system", icon: Monitor, label: t("themeSystem") },
  ];

  const changeLibrary = async () => {
    const dir = await api.pickFolder();
    if (!dir) return;
    setBusy(true);
    try {
      await api.libraryInit(dir);
      setModal(null);
      await enterMain();
      api.scanLibrary().catch(() => {});
      showToast(t("switchedLibrary", { name: dir }));
    } catch (e) {
      showToast(String(e));
    } finally {
      setBusy(false);
    }
  };

  const rebuildThumbs = async () => {
    try {
      await api.thumbsRebuild();
      setModal(null);
      showToast(t("thumbsRebuilding"));
    } catch (e) {
      showToast(String(e));
    }
  };

  return (
    <>
      <Title>{t("settings")}</Title>
      <div className="space-y-4">
        <div>
          <div className="text-[11px] font-bold text-sub mb-1.5">{t("appearance")}</div>
          <div className="flex gap-1.5">
            {themeOptions.map(({ value, icon: Icon, label }) => (
              <button
                key={value}
                onClick={() => setTheme(value)}
                className={`h-7 px-2.5 rounded-ctl text-xs border transition flex items-center gap-1.5 ${
                  theme === value
                    ? "bg-primary text-white border-primary font-bold"
                    : "bg-side border-line hover:border-primary/50"
                }`}
              >
                <Icon className="w-3.5 h-3.5" />
                {label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <div className="text-[11px] font-bold text-sub mb-1.5">{t("language")}</div>
          <div className="flex flex-wrap gap-1.5">
            {LANGS.map((l) => (
              <button
                key={l.code}
                onClick={() => setLang(l.code)}
                className={`h-7 px-2.5 rounded-ctl text-xs border transition ${
                  lang === l.code
                    ? "bg-primary text-white border-primary font-bold"
                    : "bg-side border-line hover:border-primary/50"
                }`}
              >
                {l.label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <div className="text-[11px] font-bold text-sub mb-1.5">{t("libraryLocation")}</div>
          <div className="text-xs bg-side border border-line rounded-ctl px-3 py-2 break-all select-text">{root}</div>
          <div className="mt-2 flex gap-2">
            <button
              onClick={() => api.revealFolder("").catch(() => {})}
              className="h-7 px-2.5 rounded-ctl text-xs bg-side border border-line hover:bg-line/60 transition flex items-center gap-1.5"
            >
              <FolderOpen className="w-3.5 h-3.5" />
              {t("openFolderInFinder")}
            </button>
            <button
              onClick={changeLibrary}
              disabled={busy}
              className="h-7 px-2.5 rounded-ctl text-xs bg-side border border-line hover:bg-line/60 transition disabled:opacity-50"
            >
              {t("changeLibrary")}
            </button>
          </div>
        </div>

        <div>
          <div className="text-[11px] font-bold text-sub mb-1.5">{t("maintenance")}</div>
          <div className="flex gap-2">
            <button
              onClick={rebuildThumbs}
              className="h-7 px-2.5 rounded-ctl text-xs bg-side border border-line hover:bg-line/60 transition flex items-center gap-1.5"
            >
              <Images className="w-3.5 h-3.5" />
              {t("rebuildThumbs")}
            </button>
            <button
              onClick={() => {
                api.rescan().catch(() => {});
                setModal(null);
              }}
              className="h-7 px-2.5 rounded-ctl text-xs bg-side border border-line hover:bg-line/60 transition flex items-center gap-1.5"
            >
              <RefreshCw className="w-3.5 h-3.5" />
              {t("rescanLibrary")}
            </button>
          </div>
        </div>

        <div className="text-[10.5px] text-sub leading-relaxed border-t border-line pt-3">
          {t("aboutLine")}
          <br />
          {t("aboutLine2")}
        </div>
      </div>
    </>
  );
}
