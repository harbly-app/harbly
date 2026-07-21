import { Crepe } from "@milkdown/crepe";
import { redoCommand, undoCommand } from "@milkdown/kit/plugin/history";
import { $prose, callCommand } from "@milkdown/kit/utils";
import "@milkdown/crepe/theme/common/style.css";
import "@milkdown/crepe/theme/frame.css";
import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { imageToDataUrl } from "../hdoc/image";
import { api, relAssetUrl } from "../lib/api";
import { makeT } from "../lib/i18n";
import { mdNativeImagePastePlugin, stripDoomedImageRefs } from "../lib/mdpaste";
import { useStore } from "../lib/store";
import type { AssetMeta } from "../lib/types";

const SAVE_DEBOUNCE_MS = 1000;

/** Split a leading YAML front-matter block off the body. Crepe's Markdown
 * pipeline has no front-matter support and would mangle a leading `---` block,
 * so we keep it verbatim and re-prepend it on every save. `fm + body` always
 * reconstructs the original prefix exactly. */
function splitFrontmatter(text: string): { fm: string; body: string } {
  const m = /^---\r?\n[\s\S]*?\r?\n---[ \t]*(?:\r?\n|$)/.exec(text);
  if (m?.index === 0) {
    return { fm: m[0], body: text.slice(m[0].length) };
  }
  return { fm: "", body: text };
}

/** A URL that resolves relative to the document (no scheme, not absolute) —
 * mirrors the protocol handler so relative images proxy through it. */
function isRelativeUrl(u: string): boolean {
  return (
    !!u &&
    !u.startsWith("/") &&
    !u.startsWith("#") &&
    !u.includes("://") &&
    !u.startsWith("data:") &&
    !u.startsWith("mailto:")
  );
}

/** WYSIWYG Markdown editor (Milkdown Crepe). Mounted keyed by asset id, so it is
 * never remounted by autosave (which only changes the hash). Autosaves on a 1s
 * debounce and checkpoints one version per editing session on unmount. */
export default function MarkdownEditor({ asset }: { asset: AssetMeta }) {
  const rootEl = useRef<HTMLDivElement>(null);
  const t = makeT(useStore((s) => s.lang));
  const wide = useStore((s) => s.mdWide);
  const [conflict, setConflict] = useState(false);
  // Conflict-banner actions, populated once the editor is live.
  const actions = useRef<{ reload: () => void; keepMine: () => void }>({
    reload: () => {},
    keepMine: () => {},
  });

  useEffect(() => {
    const id = asset.id;
    // The cleanup closure flips this; setup/reload re-check it after every await.
    // Read it through `gone()` so flow analysis doesn't narrow it to a constant
    // (the mutation happens in a closure the checker can't see running).
    const disposed = { v: false };
    const gone = () => disposed.v;
    let crepe: Crepe | null = null;
    const ready = { v: false };
    const dirty = { v: false };
    // Mirrors the `conflict` banner as a ref the save closures can read: while
    // an external-edit conflict is unresolved, autosave must not write.
    const conflicted = { v: false };
    const frontmatter = { v: "" };
    const lastSavedBody = { v: "" };
    const lastSavedHash = { v: asset.currentHash };
    const sessionBaseHash = { v: asset.currentHash };
    let saveTimer: ReturnType<typeof setTimeout> | null = null;
    // Serializes writes: every save chains onto the previous one, so two
    // asset_write calls for this asset never run concurrently (which could leave
    // the file and the DB hash inconsistent). `save()` resolves once its own
    // write — and all writes queued before it — have completed, so flush() waits.
    let saveChain: Promise<void> = Promise.resolve();

    const featureConfigs = {
      [Crepe.Feature.Placeholder]: { text: t("mdPlaceholder") },
      [Crepe.Feature.ImageBlock]: {
        proxyDomURL: (url: string) =>
          isRelativeUrl(url) ? relAssetUrl(id, url) : url,
        // Without this, Crepe stores pasted/dropped images as
        // URL.createObjectURL blob: URLs — they render this session and are
        // gone forever after a reload. Embed as data: URLs (with downscaling),
        // same pipeline as the hdoc editor.
        onUpload: imageToDataUrl,
      },
    };

    const buildCrepe = (body: string): Crepe => {
      const c = new Crepe({
        root: rootEl.current,
        defaultValue: body,
        // KaTeX is out of scope for v1; every other Typora-like feature stays on
        features: { [Crepe.Feature.Latex]: false },
        featureConfigs,
      });
      // Registered after Crepe's clipboard/upload plugins, so it only sees the
      // pastes they decline: raw macOS clipboard bitmaps invisible to JS.
      c.editor.use($prose(() => mdNativeImagePastePlugin()));
      c.on((listener) =>
        listener.markdownUpdated((_ctx, markdown) => {
          if (!ready.v || markdown === lastSavedBody.v) return;
          dirty.v = true;
          // gone(): a late async mutation (image upload resolving) must not
          // resurrect the badge after unmount cleanup reset it to null.
          if (!gone()) useStore.getState().setSaveState("editing");
          scheduleSave();
        }),
      );
      return c;
    };

    const scheduleSave = () => {
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = setTimeout(() => void save(), SAVE_DEBOUNCE_MS);
    };

    const doWrite = async (force: boolean) => {
      if (!crepe || !ready.v) return;
      // An unresolved external-edit conflict freezes autosave: writing our copy
      // now would overwrite the other edit before the user decides. Only the
      // explicit "keep mine" path (force) is allowed through.
      if (conflicted.v && !force) return;
      const body = stripDoomedImageRefs(crepe.getMarkdown());
      if (!force && body === lastSavedBody.v) {
        dirty.v = false;
        // The unmount flush also lands here — after cleanup already reset the
        // global saveState to null, it must not be resurrected as "saved"
        // (it would badge unrelated files, or mask a newer editor's state).
        if (!gone()) useStore.getState().setSaveState("saved");
        return;
      }
      try {
        const meta = await api.assetWrite(id, frontmatter.v + body);
        lastSavedBody.v = body;
        lastSavedHash.v = meta.currentHash;
        dirty.v = false;
        if (!gone()) useStore.getState().setSaveState("saved");
        // Reflect the new hash on the open asset so our own write is recognized
        // as an echo (not an external change) and the title stays current.
        useStore.setState((s) =>
          s.viewerAsset?.id === meta.id ? { viewerAsset: meta } : {},
        );
      } catch {
        // Keep `dirty` set so the next edit / flush retries, and surface the
        // failure — a silent drop would lose the edit when the editor closes.
        useStore.getState().showToast(t("mdSaveFailed"));
      }
    };

    // Persist the current buffer, serialized behind any in-flight save. `force`
    // bypasses the no-op guard (used when keeping the user's version on conflict).
    const save = (force = false): Promise<void> => {
      if (saveTimer) {
        clearTimeout(saveTimer);
        saveTimer = null;
      }
      saveChain = saveChain.then(() => doWrite(force));
      return saveChain;
    };

    // Adopt the on-disk version, discarding the editor buffer (external change
    // with no local edits, or the user chose "load disk version").
    const reloadFromDisk = async () => {
      const text = await api.assetReadText(id).catch(() => null);
      if (gone() || text === null) return;
      const { fm, body } = splitFrontmatter(text);
      frontmatter.v = fm;
      ready.v = false;
      if (crepe) {
        try {
          await crepe.destroy();
        } catch {
          /* already torn down */
        }
      }
      if (gone()) return;
      crepe = buildCrepe(body);
      await crepe.create();
      if (gone()) {
        void crepe.destroy();
        return;
      }
      lastSavedBody.v = crepe.getMarkdown();
      lastSavedHash.v =
        useStore.getState().viewerAsset?.currentHash ?? lastSavedHash.v;
      sessionBaseHash.v = lastSavedHash.v; // the external edit is already its own version
      dirty.v = false;
      conflicted.v = false;
      setConflict(false);
      ready.v = true;
      useStore.getState().setSaveState("saved");
    };

    const undo = () => crepe?.editor.action(callCommand(undoCommand.key));
    const redo = () => crepe?.editor.action(callCommand(redoCommand.key));
    const flush = async () => {
      await save();
    };
    actions.current = {
      reload: () => void reloadFromDisk(),
      keepMine: () => {
        conflicted.v = false;
        setConflict(false);
        void save(true);
      },
    };

    // React to external changes to this asset. asset_write does not emit
    // library-changed, so viewerAsset.currentHash only moves on our own save
    // (echo, ignored) or a real external edit picked up by the watcher → refresh.
    const unsub = useStore.subscribe((s) => {
      const cur = s.viewerAsset;
      if (cur?.id !== id || !ready.v) return;
      if (cur.currentHash === lastSavedHash.v) return; // our echo
      if (dirty.v) {
        // Cancel the in-flight debounced autosave so it can't land on top of
        // the external edit while the conflict banner is up.
        if (saveTimer) {
          clearTimeout(saveTimer);
          saveTimer = null;
        }
        conflicted.v = true;
        setConflict(true);
      } else void reloadFromDisk();
    });

    const onVisibility = () => {
      if (document.visibilityState === "hidden") void save();
    };
    document.addEventListener("visibilitychange", onVisibility);

    void (async () => {
      const text = await api.assetReadText(id).catch(() => "");
      if (gone()) return;
      const { fm, body } = splitFrontmatter(text);
      frontmatter.v = fm;
      crepe = buildCrepe(body);
      await crepe.create();
      if (gone()) {
        void crepe.destroy();
        crepe = null;
        return;
      }
      // Reconcile against Crepe's own serialization so the initial normalization
      // pass is not mistaken for a user edit (open never rewrites the file).
      lastSavedBody.v = crepe.getMarkdown();
      ready.v = true;
      useStore.getState().setEditorHandle({ undo, redo, flush });
      useStore.getState().setSaveState("saved");
    })();

    return () => {
      disposed.v = true;
      unsub();
      document.removeEventListener("visibilitychange", onVisibility);
      if (saveTimer) clearTimeout(saveTimer);
      useStore.getState().setEditorHandle(null);
      useStore.getState().setSaveState(null);
      const dying = crepe;
      // An unresolved conflict froze autosave, so the flush below will bail —
      // and the buffer is about to die with the editor. Rescue the local
      // edits as a version snapshot (never the live file: the user hasn't
      // chosen a side of the conflict). Serialize NOW, synchronously, while
      // the editor is still alive.
      let rescue: string | null = null;
      if (conflicted.v && dirty.v && crepe) {
        try {
          rescue = frontmatter.v + crepe.getMarkdown();
        } catch {
          /* already torn down */
        }
      }
      // Flush the pending edit FIRST, while `crepe`/`ready` are still valid —
      // clearing them up front would make doWrite bail and silently drop an edit
      // typed within the autosave debounce when switching documents. Only after
      // the flush do we stop accepting writes, checkpoint the session, and destroy.
      void (async () => {
        if (rescue !== null) {
          try {
            await api.assetSnapshotText(id, rescue);
          } catch {
            /* the file may have been deleted mid-session */
          }
        }
        try {
          await save();
        } catch {
          /* failure already surfaced via toast in doWrite */
        }
        ready.v = false;
        crepe = null;
        try {
          await api.assetCheckpoint(id, sessionBaseHash.v);
        } catch {
          /* noop */
        }
        if (dying) {
          try {
            await dying.destroy();
          } catch {
            /* noop */
          }
        }
      })();
    };
    // Keyed by id: switching files remounts; hash changes must NOT remount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asset.id]);

  return (
    <main className="harbly-md relative flex min-w-0 flex-1 flex-col bg-paper">
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
      <div className="min-h-0 flex-1 overflow-y-auto">
        {/* Vertical rhythm + side insets come from the editor's own padding
            (tuned in styles.css). Comfortable mode caps the reading column;
            wide mode lets it fill the pane (better for tables / code). */}
        <div
          ref={rootEl}
          className={`mx-auto w-full ${wide ? "" : "max-w-[52rem]"}`}
        />
      </div>
    </main>
  );
}
