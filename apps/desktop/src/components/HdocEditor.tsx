import { RefreshCw } from "lucide-react";
import { baseKeymap } from "prosemirror-commands";
import { dropCursor } from "prosemirror-dropcursor";
import { gapCursor } from "prosemirror-gapcursor";
import { history, redo, undo } from "prosemirror-history";
import { keymap } from "prosemirror-keymap";
import { DOMSerializer, Fragment, Slice } from "prosemirror-model";
import { EditorState } from "prosemirror-state";
import { tableEditing } from "prosemirror-tables";
import { EditorView } from "prosemirror-view";
import { Fragment as ReactFragment, useEffect, useRef, useState } from "react";
import { api, assetUrl } from "../lib/api";
import { makeT } from "../lib/i18n";
import type { TFn } from "../lib/i18n";
import { useStore } from "../lib/store";
import type { AssetMeta } from "../lib/types";
import { dragHandle } from "../hdoc/draghandle";
import { hdocItems } from "../hdoc/items";
import type { HdocItem } from "../hdoc/items";
import { hdocNodeViews } from "../hdoc/nodeviews";
import { parseHdoc } from "../hdoc/parse";
import { hdocInputRules, hdocKeymap, hdocPlaceholders } from "../hdoc/plugins";
import { hdocSchema, THEMES } from "../hdoc/schema";
import { serializeHdoc } from "../hdoc/serialize";
import { slashMenu } from "../hdoc/slash";
import "prosemirror-view/style/prosemirror.css";
import "prosemirror-gapcursor/style/gapcursor.css";
import "prosemirror-tables/style/tables.css";
import "../../src-tauri/assets/hdoc/runtime.css";

const SAVE_DEBOUNCE_MS = 1000;

/** WYSIWYG page editor (ProseMirror on a real <h-doc> element, so the shared
 * runtime CSS makes the editor render exactly like the page). Mounted keyed by
 * asset id; autosaves on a 1s debounce; checkpoints one version per session on
 * unmount — the exact lifecycle of MarkdownEditor. Documents containing
 * content outside the v1 vocabulary open as a read-only preview instead, so a
 * save can never destroy what the editor doesn't understand. */
export default function HdocEditor({ asset }: { asset: AssetMeta }) {
  const wrapEl = useRef<HTMLDivElement>(null);
  const pmViewRef = useRef<EditorView | null>(null);
  const t = makeT(useStore((s) => s.lang));
  const wide = useStore((s) => s.mdWide);
  const [conflict, setConflict] = useState(false);
  const [unsupported, setUnsupported] = useState(false);
  const [theme, setTheme] = useState("paper");
  const actions = useRef<{
    reload: () => void;
    keepMine: () => void;
    setTheme: (v: string) => void;
  }>({ reload: () => {}, keepMine: () => {}, setTheme: () => {} });

  useEffect(() => {
    const id = asset.id;
    const disposed = { v: false };
    const gone = () => disposed.v;
    let view: EditorView | null = null;
    let hdocEl: HTMLElement | null = null;
    const ready = { v: false };
    const dirty = { v: false };
    const lastSavedBody = { v: "" };
    const lastSavedHash = { v: asset.currentHash };
    const sessionBaseHash = { v: asset.currentHash };
    let saveTimer: ReturnType<typeof setTimeout> | null = null;
    // Serializes writes (see MarkdownEditor): saves chain so two asset_write
    // calls for this asset never run concurrently, and flush() awaits them all.
    let saveChain: Promise<void> = Promise.resolve();

    const plugins = [
      hdocInputRules(),
      hdocKeymap(),
      keymap({ "Mod-z": undo, "Shift-Mod-z": redo, "Mod-y": redo }),
      keymap(baseKeymap),
      dropCursor({ class: "hd-dropcursor" }),
      gapCursor(),
      history(),
      tableEditing(),
      slashMenu(),
      dragHandle(),
      hdocPlaceholders(),
    ];

    const buildView = (doc: EditorState["doc"]): EditorView | null => {
      const wrap = wrapEl.current;
      if (!wrap) return null;
      hdocEl = document.createElement("h-doc");
      wrap.replaceChildren(hdocEl);
      const state = EditorState.create({ doc, plugins });
      const v = new EditorView(
        { mount: hdocEl },
        {
          state,
          nodeViews: hdocNodeViews(id),
          dispatchTransaction: (trx) => {
            if (!view) return;
            const newState = view.state.apply(trx);
            view.updateState(newState);
            const th = String(newState.doc.attrs.theme ?? "paper");
            hdocEl?.setAttribute("theme", th);
            setTheme(th);
            if (trx.docChanged && ready.v) {
              dirty.v = true;
              scheduleSave();
            }
          },
        },
      );
      hdocEl.setAttribute("theme", String(doc.attrs.theme ?? "paper"));
      setTheme(String(doc.attrs.theme ?? "paper"));
      return v;
    };

    const scheduleSave = () => {
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = setTimeout(() => void save(), SAVE_DEBOUNCE_MS);
    };

    const doWrite = async (force: boolean) => {
      if (!view || !ready.v) return;
      const body = serializeHdoc(view.state.doc);
      if (!force && body === lastSavedBody.v) {
        dirty.v = false;
        return;
      }
      try {
        const meta = await api.assetWrite(id, body);
        lastSavedBody.v = body;
        lastSavedHash.v = meta.currentHash;
        dirty.v = false;
        useStore.setState((s) =>
          s.viewerAsset?.id === meta.id ? { viewerAsset: meta } : {},
        );
      } catch {
        useStore.getState().showToast(t("mdSaveFailed"));
      }
    };

    const save = (force = false): Promise<void> => {
      if (saveTimer) {
        clearTimeout(saveTimer);
        saveTimer = null;
      }
      saveChain = saveChain.then(() => doWrite(force));
      return saveChain;
    };

    const loadIntoView = (text: string): boolean => {
      const parsed = parseHdoc(text);
      if (!parsed.ok) {
        setUnsupported(true);
        return false;
      }
      setUnsupported(false);
      if (view) {
        view.updateState(EditorState.create({ doc: parsed.doc, plugins }));
        hdocEl?.setAttribute(
          "theme",
          String(parsed.doc.attrs.theme ?? "paper"),
        );
        setTheme(String(parsed.doc.attrs.theme ?? "paper"));
      } else {
        view = buildView(parsed.doc);
        if (!view) return false;
      }
      pmViewRef.current = view;
      lastSavedBody.v = serializeHdoc(view.state.doc);
      return true;
    };

    const reloadFromDisk = async () => {
      const text = await api.assetReadText(id).catch(() => null);
      if (gone() || text === null) return;
      ready.v = false;
      if (!loadIntoView(text)) return;
      lastSavedHash.v =
        useStore.getState().viewerAsset?.currentHash ?? lastSavedHash.v;
      sessionBaseHash.v = lastSavedHash.v; // the external edit is already its own version
      dirty.v = false;
      ready.v = true;
      setConflict(false);
    };

    const doUndo = () => {
      if (view) undo(view.state, view.dispatch);
    };
    const doRedo = () => {
      if (view) redo(view.state, view.dispatch);
    };
    const flush = async () => {
      await save();
    };
    actions.current = {
      reload: () => void reloadFromDisk(),
      keepMine: () => {
        setConflict(false);
        void save(true);
      },
      setTheme: (v: string) => {
        if (!view) return;
        view.dispatch(view.state.tr.setDocAttribute("theme", v));
      },
    };

    // External-change detection: our own saves echo back as the same hash.
    const unsub = useStore.subscribe((s) => {
      const cur = s.viewerAsset;
      if (cur?.id !== id || !ready.v) return;
      if (cur.currentHash === lastSavedHash.v) return; // our echo
      if (dirty.v) setConflict(true);
      else void reloadFromDisk();
    });

    const onVisibility = () => {
      if (document.visibilityState === "hidden") void save();
    };
    document.addEventListener("visibilitychange", onVisibility);

    void (async () => {
      const text = await api.assetReadText(id).catch(() => "");
      if (gone()) return;
      if (!loadIntoView(text)) return; // unsupported → read-only preview
      ready.v = true;
      useStore
        .getState()
        .setEditorHandle({ undo: doUndo, redo: doRedo, flush });
    })();

    return () => {
      disposed.v = true;
      unsub();
      document.removeEventListener("visibilitychange", onVisibility);
      if (saveTimer) clearTimeout(saveTimer);
      useStore.getState().setEditorHandle(null);
      pmViewRef.current = null;
      const dying = view;
      // Flush first while the view is still valid, then checkpoint & destroy
      // (same ordering rationale as MarkdownEditor).
      void (async () => {
        try {
          await save();
        } catch {
          /* surfaced via toast in doWrite */
        }
        ready.v = false;
        view = null;
        try {
          await api.assetCheckpoint(id, sessionBaseHash.v);
        } catch {
          /* the file may have been deleted mid-session */
        }
        dying?.destroy();
      })();
    };
    // Keyed by id: switching files remounts; hash changes must NOT remount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asset.id]);

  if (unsupported) {
    return (
      <main className="relative flex min-w-0 flex-1 flex-col bg-paper">
        <div className="flex items-center gap-3 border-b border-line bg-warn/15 px-5 py-2.5 text-xs">
          <RefreshCw className="h-3.5 w-3.5 shrink-0 text-warn" />
          <span className="flex-1 text-ink">{t("hdocUnsupported")}</span>
        </div>
        <div className="relative flex-1 bg-white">
          <iframe
            src={assetUrl(asset.id)}
            sandbox="allow-scripts allow-same-origin"
            className="absolute inset-0 h-full w-full border-0"
            title={asset.title}
          />
        </div>
      </main>
    );
  }

  return (
    <main
      className={`hdoc-editor relative flex min-w-0 flex-1 flex-col bg-paper ${wide ? "hdoc-wide" : ""}`}
    >
      {conflict && (
        <div className="absolute inset-x-0 top-0 z-10 flex items-center gap-3 border-b border-line bg-warn/15 px-5 py-2.5 text-xs">
          <RefreshCw className="h-3.5 w-3.5 shrink-0 text-warn" />
          <span className="flex-1 text-ink">{t("mdConflictTitle")}</span>
          <button
            onClick={() => actions.current.keepMine()}
            className="rounded-ctl px-2.5 py-1 font-bold text-sub2 transition hover:bg-side"
          >
            {t("mdKeepMine")}
          </button>
          <button
            onClick={() => actions.current.reload()}
            className="rounded-ctl bg-primary px-2.5 py-1 font-bold text-white transition hover:bg-primary-light"
          >
            {t("mdLoadDisk")}
          </button>
        </div>
      )}
      <InsertToolbar
        viewRef={pmViewRef}
        t={t}
        themeSel={
          <ThemeSelect
            theme={theme}
            t={t}
            onChange={(v) => actions.current.setTheme(v)}
          />
        }
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div ref={wrapEl} className="hdoc-wrap relative min-h-full" />
      </div>
    </main>
  );
}

/** Document theme is a property of the file, not the app appearance. */
function ThemeSelect({
  theme,
  t,
  onChange,
}: {
  theme: string;
  t: TFn;
  onChange: (v: string) => void;
}) {
  return (
    <select
      value={theme}
      onChange={(e) => onChange(e.target.value)}
      title={t("hdocTheme")}
      className="rounded-ctl border border-line bg-card px-2 py-1 text-[11px] text-sub transition outline-none hover:text-ink"
    >
      {THEMES.map((v) => (
        <option key={v} value={v}>
          {t(
            v === "paper"
              ? "themePaper"
              : v === "sepia"
                ? "themeSepia"
                : "themeNight",
          )}
        </option>
      ))}
    </select>
  );
}

/** Insert toolbar across the editor top: icon buttons (hover for the name),
 * click inserts at the cursor, drag places the block exactly where it drops —
 * the drag hands ProseMirror a ready slice via `view.dragging`, and its own
 * drop logic + dropcursor do the rest. */
function InsertToolbar({
  viewRef,
  t,
  themeSel,
}: {
  viewRef: React.RefObject<EditorView | null>;
  t: TFn;
  themeSel: React.ReactNode;
}) {
  const items = hdocItems();
  const groups: HdocItem["group"][] = ["basic", "component"];

  const onDragStart = (e: React.DragEvent, it: HdocItem) => {
    const v = viewRef.current;
    if (!v) return;
    const node = it.make();
    const slice = new Slice(Fragment.from(node), 0, 0);
    const frag = DOMSerializer.fromSchema(hdocSchema).serializeFragment(
      slice.content,
    );
    const div = document.createElement("div");
    div.appendChild(frag);
    e.dataTransfer.setData("text/html", div.innerHTML);
    e.dataTransfer.effectAllowed = "copy";
    v.dragging = { slice, move: false };
  };
  const onDragEnd = () => {
    const v = viewRef.current;
    if (v) v.dragging = null;
  };

  return (
    <div className="flex h-10 shrink-0 items-center gap-0.5 overflow-x-auto border-b border-line bg-paper px-3">
      {groups.map((g, gi) => (
        <ReactFragment key={g}>
          {gi > 0 && <div className="mx-1.5 h-4 w-px shrink-0 bg-line" />}
          {items
            .filter((it) => it.group === g)
            .map((it) => (
              <button
                key={it.key}
                title={t(it.labelKey)}
                draggable
                onDragStart={(e) => onDragStart(e, it)}
                onDragEnd={onDragEnd}
                onClick={() => {
                  const v = viewRef.current;
                  if (v) it.run(v);
                }}
                className="grid h-7 w-7 shrink-0 cursor-grab place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
              >
                <span
                  className="hd-ico"
                  dangerouslySetInnerHTML={{ __html: it.icon }}
                />
              </button>
            ))}
        </ReactFragment>
      ))}
      <div className="min-w-3 flex-1" />
      {themeSel}
    </div>
  );
}
