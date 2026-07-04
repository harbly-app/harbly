import {
  Folder,
  FolderOpen,
  Hash,
  Images,
  Inbox as InboxIcon,
  Monitor,
  Moon,
  RefreshCw,
  Sun,
} from "lucide-react";
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
      className="fixed inset-0 z-50 grid place-items-center bg-black/30"
      onMouseDown={() => setModal(null)}
    >
      <div
        className="w-[420px] rounded-card border border-line bg-card p-5 shadow-2xl"
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
  return <div className="mb-3 text-[14px] font-extrabold">{children}</div>;
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
        className="h-8 rounded-ctl border border-line bg-side px-3.5 text-xs font-bold transition hover:bg-line/60"
      >
        {t("cancel")}
      </button>
      <button
        autoFocus={props.autoFocusOk}
        onClick={props.onOk}
        disabled={props.okDisabled}
        className={`h-8 rounded-ctl px-3.5 text-xs font-bold text-white transition disabled:opacity-40 ${
          props.danger
            ? "bg-danger hover:opacity-90"
            : "bg-primary hover:bg-primary-light"
        }`}
      >
        {props.okLabel}
      </button>
    </div>
  );
}

function flatten(
  tree: TreeNode | null,
  rootLabel: string,
): { rel: string; label: string; depth: number }[] {
  const out: { rel: string; label: string; depth: number }[] = [
    { rel: "", label: rootLabel, depth: 0 },
  ];
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
    (x) => m.fromFolder == null || x.rel !== m.fromFolder,
  );

  return (
    <>
      <Title>{t("moveTitle", { name: m.label })}</Title>
      <div className="-mx-1.5 max-h-[300px] space-y-0.5 overflow-y-auto px-1.5">
        {targets.map((x) => (
          <button
            key={x.rel}
            onClick={() => doMove(m.ids, x.rel)}
            className="flex w-full items-center gap-2 rounded-ctl px-2.5 py-2 text-[12.5px] transition hover:bg-primary/10 hover:text-primary"
            style={{ paddingLeft: 10 + x.depth * 16 }}
          >
            {x.rel === "" ? (
              <InboxIcon className="h-3.5 w-3.5 opacity-0" />
            ) : (
              <Folder className="h-3.5 w-3.5 text-sub" />
            )}
            <span className="truncate">{x.label}</span>
          </button>
        ))}
      </div>
      <div className="mt-2 text-[10.5px] text-sub">{t("moveHint")}</div>
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
        className="h-9 w-full rounded-ctl border border-line bg-side px-3 text-[13px] outline-none focus:border-primary"
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
    void doDeleteFolder(m.rel);
  };

  return (
    <>
      <Title>{t("confirmDelFolderTitle")}</Title>
      <div className="text-[12.5px] leading-relaxed text-sub2">
        {t("confirmDelFolderBody", { name: m.label })}
      </div>
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
  const [selected, setSelected] = useState<Set<string>>(
    new Set(asset?.tags ?? []),
  );
  const [input, setInput] = useState("");

  if (!asset) return null;

  const candidates = Array.from(
    new Set([...allTags.map((x) => x.name), ...selected]),
  );

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
      showToast(
        final.size > 0 ? t("savedTags", { n: final.size }) : t("clearedTags"),
      );
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
          else void ok();
        }}
        placeholder={t("tagsPlaceholder")}
        className="h-9 w-full rounded-ctl border border-line bg-side px-3 text-[13px] outline-none focus:border-primary"
      />
      <div className="mt-3 flex max-h-[200px] flex-wrap gap-1.5 overflow-y-auto">
        {candidates.length === 0 && (
          <span className="text-xs text-sub">{t("noTagsYet")}</span>
        )}
        {candidates.map((x) => {
          const on = selected.has(x);
          return (
            <button
              key={x}
              onClick={() => toggle(x)}
              className={`flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11.5px] font-bold transition ${
                on
                  ? "border-primary bg-primary text-white"
                  : "border-line bg-card text-sub2 hover:border-primary/50"
              }`}
            >
              <Hash className="h-3 w-3" />
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
  const themeOptions: { value: ThemePref; icon: typeof Sun; label: string }[] =
    [
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
          <div className="mb-1.5 text-[11px] font-bold text-sub">
            {t("appearance")}
          </div>
          <div className="flex gap-1.5">
            {themeOptions.map(({ value, icon: Icon, label }) => (
              <button
                key={value}
                onClick={() => setTheme(value)}
                className={`flex h-7 items-center gap-1.5 rounded-ctl border px-2.5 text-xs transition ${
                  theme === value
                    ? "border-primary bg-primary font-bold text-white"
                    : "border-line bg-side hover:border-primary/50"
                }`}
              >
                <Icon className="h-3.5 w-3.5" />
                {label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <div className="mb-1.5 text-[11px] font-bold text-sub">
            {t("language")}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {LANGS.map((l) => (
              <button
                key={l.code}
                onClick={() => setLang(l.code)}
                className={`h-7 rounded-ctl border px-2.5 text-xs transition ${
                  lang === l.code
                    ? "border-primary bg-primary font-bold text-white"
                    : "border-line bg-side hover:border-primary/50"
                }`}
              >
                {l.label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <div className="mb-1.5 text-[11px] font-bold text-sub">
            {t("libraryLocation")}
          </div>
          <div className="rounded-ctl border border-line bg-side px-3 py-2 text-xs break-all select-text">
            {root}
          </div>
          <div className="mt-2 flex gap-2">
            <button
              onClick={() => api.revealFolder("").catch(() => {})}
              className="flex h-7 items-center gap-1.5 rounded-ctl border border-line bg-side px-2.5 text-xs transition hover:bg-line/60"
            >
              <FolderOpen className="h-3.5 w-3.5" />
              {t("openFolderInFinder")}
            </button>
            <button
              onClick={changeLibrary}
              disabled={busy}
              className="h-7 rounded-ctl border border-line bg-side px-2.5 text-xs transition hover:bg-line/60 disabled:opacity-50"
            >
              {t("changeLibrary")}
            </button>
          </div>
        </div>

        <div>
          <div className="mb-1.5 text-[11px] font-bold text-sub">
            {t("maintenance")}
          </div>
          <div className="flex gap-2">
            <button
              onClick={rebuildThumbs}
              className="flex h-7 items-center gap-1.5 rounded-ctl border border-line bg-side px-2.5 text-xs transition hover:bg-line/60"
            >
              <Images className="h-3.5 w-3.5" />
              {t("rebuildThumbs")}
            </button>
            <button
              onClick={() => {
                api.rescan().catch(() => {});
                setModal(null);
              }}
              className="flex h-7 items-center gap-1.5 rounded-ctl border border-line bg-side px-2.5 text-xs transition hover:bg-line/60"
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t("rescanLibrary")}
            </button>
          </div>
        </div>

        <div className="border-t border-line pt-3 text-[10.5px] leading-relaxed text-sub">
          {t("aboutLine")}
          <br />
          {t("aboutLine2")}
        </div>
      </div>
    </>
  );
}
