import * as CM from "@radix-ui/react-context-menu";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ClipboardCopy,
  Copy,
  Download,
  ExternalLink,
  Eye,
  FileCode2,
  FolderInput,
  FolderOpen,
  PencilLine,
  Tag as TagIcon,
  Trash2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, thumbUrl, timeAgo } from "../lib/api";
import { makeT } from "../lib/i18n";
import { dragJustEnded, useStore } from "../lib/store";
import type { DragPayload } from "../lib/store";
import type { AssetMeta, SortKey } from "../lib/types";
import { INBOX } from "../lib/types";
import { dragStartHandler } from "../lib/drag";
import { menuContentCls, MItem, MSep } from "./menu";
import RenameInput from "./RenameInput";

const CARD_MIN = 216;
const GAP = 14;
const PAD = 20;

const stem = (fileName: string) => fileName.replace(/\.(html?|htm)$/i, "");

/** Build the drag payload from the latest selection at mousedown: dragging an already-selected item drags the whole group (Finder semantics) */
function payloadFor(a: AssetMeta): DragPayload {
  const st = useStore.getState();
  const ids =
    st.selIds.includes(a.id) && st.selIds.length > 1 ? st.selIds : [a.id];
  const byId = new Map(st.assets.map((x) => [x.id, x]));
  return {
    ids,
    rels: ids.map((i) => byId.get(i)?.relPath ?? "").filter(Boolean),
    label: a.fileName,
    fromFolder: a.folder,
  };
}

export default function AssetGrid() {
  const assets = useStore((s) => s.assets);
  const folder = useStore((s) => s.folder);
  const sort = useStore((s) => s.sort);
  const setSort = useStore((s) => s.setSort);
  const selCount = useStore((s) => s.selIds.length);

  const scrollRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(900);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const inner = Math.max(320, width - PAD * 2);
  const cols = Math.max(
    2,
    Math.min(6, Math.floor((inner + GAP) / (CARD_MIN + GAP))),
  );
  const cardW = (inner - GAP * (cols - 1)) / cols;
  const rowH = Math.round(cardW * 0.72) + 62 + GAP;
  const rows = Math.ceil(assets.length / cols);

  const virt = useVirtualizer({
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowH,
    overscan: 4,
  });

  useEffect(() => {
    virt.measure();
  }, [rowH, virt]);

  // Latest snapshots for the global keyboard handler to read
  const listRef = useRef(assets);
  listRef.current = assets;
  const colsRef = useRef(cols);
  colsRef.current = cols;
  const virtRef = useRef(virt);
  virtRef.current = virt;

  // Global keys (Finder semantics): arrows move selection, Shift+arrows extend, Enter renames,
  // Space / Cmd+Down opens, Cmd+Backspace deletes, Esc clears selection
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      if (
        st.viewerAsset ||
        st.paletteOpen ||
        st.modal ||
        st.editingAsset ||
        st.editingFolder
      )
        return;
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.isContentEditable)
      )
        return;
      const list = listRef.current;
      const cols = colsRef.current;
      const lead = st.selIds.length ? st.selIds[st.selIds.length - 1] : null;
      const leadIdx = lead ? list.findIndex((a) => a.id === lead) : -1;

      if (e.metaKey && e.key === "ArrowDown") {
        // Cmd+Down = open (Finder)
        if (lead) {
          e.preventDefault();
          st.openViewer(lead);
        }
        return;
      }
      const arrows: Record<string, number> = {
        ArrowLeft: -1,
        ArrowRight: 1,
        ArrowUp: -cols,
        ArrowDown: cols,
      };
      if (e.key in arrows && !e.metaKey) {
        e.preventDefault();
        if (!list.length) return;
        let n: number;
        if (leadIdx < 0) {
          n = 0;
        } else {
          n = leadIdx + arrows[e.key];
          if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
            n = Math.max(0, Math.min(list.length - 1, n));
          } else if (n < 0 || n >= list.length) {
            return;
          }
        }
        const target = list[n].id;
        if (e.shiftKey && st.anchorId) {
          const ai = list.findIndex((a) => a.id === st.anchorId);
          if (ai >= 0) {
            const [lo, hi] = ai <= n ? [ai, n] : [n, ai];
            // Keep target last so it stays the lead
            const range = list
              .slice(lo, hi + 1)
              .map((a) => a.id)
              .filter((id) => id !== target);
            st.setSel([...range, target]);
          } else {
            st.setSel([target], target);
          }
        } else {
          st.setSel([target], target);
        }
        virtRef.current.scrollToIndex(Math.floor(n / cols), { align: "auto" });
      } else if (e.key === "Enter" && st.selIds.length === 1) {
        // Enter = rename in place (Finder); open via double-click / Space / Cmd+Down
        e.preventDefault();
        st.startEditAsset(st.selIds[0]);
      } else if (e.key === " " && lead) {
        e.preventDefault();
        st.openViewer(lead);
      } else if (
        e.key === "Backspace" &&
        (e.metaKey || e.ctrlKey) &&
        st.selIds.length
      ) {
        e.preventDefault();
        void st.doTrash(st.selIds);
      } else if (e.key === "Escape") {
        st.setSel([], null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const t = makeT(useStore((s) => s.lang));
  const isTag = folder.startsWith("#");
  const title =
    folder === INBOX
      ? t("inbox")
      : folder === ""
        ? t("allAssets")
        : isTag
          ? folder
          : folder.split("/").pop();
  const parent =
    !isTag && folder !== INBOX && folder.includes("/")
      ? folder.slice(0, folder.lastIndexOf("/"))
      : "";

  // Clicking whitespace between/below cards = clear selection (Finder)
  const clearIfSelf = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) useStore.getState().setSel([], null);
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col bg-paper">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-line px-5">
        <div className="flex min-w-0 items-baseline gap-1.5">
          {parent && (
            <span className="truncate text-xs text-sub">{parent} /</span>
          )}
          <span className="truncate text-[15px] font-extrabold">{title}</span>
          <span className="ml-1 text-xs text-sub">
            {t("itemsCount", { n: assets.length })}
            {selCount > 1 ? ` · ${t("selectedCount", { n: selCount })}` : ""}
          </span>
          {folder === INBOX && assets.length > 0 && (
            <span className="ml-1 text-xs text-sub">{t("inboxHint")}</span>
          )}
        </div>
        <div className="flex-1" />
        {!isTag && (
          <button
            onClick={() =>
              api
                .revealFolder(folder === INBOX ? "_inbox" : folder)
                .catch(() => {})
            }
            className="flex h-7 items-center gap-1.5 rounded-ctl px-2.5 text-xs text-sub2 transition hover:bg-side"
          >
            <FolderOpen className="h-3.5 w-3.5" />
            {t("openFolderInFinder")}
          </button>
        )}
        <select
          value={sort}
          onChange={(e) => setSort(e.target.value as SortKey)}
          className="h-7 rounded-ctl border border-line bg-side px-2 text-xs outline-none"
        >
          <option value="recent">{t("sortRecent")}</option>
          <option value="modified">{t("sortModified")}</option>
          <option value="name">{t("sortName")}</option>
        </select>
      </div>

      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto"
        onMouseDown={clearIfSelf}
      >
        {assets.length === 0 ? (
          <EmptyState inbox={folder === INBOX} />
        ) : (
          <div
            className="relative mx-5 my-4"
            style={{ height: virt.getTotalSize() }}
            onMouseDown={clearIfSelf}
          >
            {virt.getVirtualItems().map((row) => (
              <div
                key={row.key}
                className="absolute right-0 left-0 flex"
                style={{ top: row.start, gap: GAP }}
                onMouseDown={clearIfSelf}
              >
                {assets
                  .slice(row.index * cols, row.index * cols + cols)
                  .map((a) => (
                    <Card key={a.id} a={a} w={cardW} inbox={folder === INBOX} />
                  ))}
              </div>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}

function EmptyState({ inbox }: { inbox: boolean }) {
  const t = makeT(useStore((s) => s.lang));
  return (
    <div className="grid h-full place-items-center">
      <div className="rounded-card border-2 border-dashed border-line px-14 py-12 text-center">
        <FileCode2 className="mx-auto mb-3 h-8 w-8 text-sub" />
        <div className="text-sm font-bold text-sub2">
          {inbox ? t("emptyInboxTitle") : t("emptyTitle")}
        </div>
        <div className="mt-1.5 text-xs text-sub">
          {inbox ? t("emptyInboxDesc") : t("emptyDesc")}
        </div>
      </div>
    </div>
  );
}

function Thumb({ hash, epoch }: { hash: string; epoch: number }) {
  const [failed, setFailed] = useState(false);
  if (failed) {
    return (
      <div className="grid h-full w-full place-items-center text-sub">
        <FileCode2 className="h-7 w-7" />
      </div>
    );
  }
  return (
    <img
      src={`${thumbUrl(hash)}?e=${epoch}`}
      onError={() => setFailed(true)}
      loading="lazy"
      draggable={false}
      className="h-full w-full object-cover object-top"
      alt=""
    />
  );
}

/** Finder-style selection: click to select · Cmd-click to toggle · Shift-click for range · double-click to open */
function handleSelectClick(a: AssetMeta, e: React.MouseEvent) {
  const st = useStore.getState();
  if (e.metaKey) {
    const has = st.selIds.includes(a.id);
    st.setSel(
      has ? st.selIds.filter((x) => x !== a.id) : [...st.selIds, a.id],
      a.id,
    );
  } else if (e.shiftKey && st.anchorId) {
    const list = st.assets;
    const ai = list.findIndex((x) => x.id === st.anchorId);
    const ni = list.findIndex((x) => x.id === a.id);
    if (ai >= 0 && ni >= 0) {
      const [lo, hi] = ai <= ni ? [ai, ni] : [ni, ai];
      const range = list
        .slice(lo, hi + 1)
        .map((x) => x.id)
        .filter((id) => id !== a.id);
      st.setSel([...range, a.id]);
    } else {
      st.setSel([a.id], a.id);
    }
  } else {
    st.setSel([a.id], a.id);
  }
}

function Card({ a, w, inbox }: { a: AssetMeta; w: number; inbox: boolean }) {
  const selected = useStore((s) => s.selIds.includes(a.id));
  const editing = useStore((s) => s.editingAsset === a.id);
  const multi = useStore((s) => s.selIds.length > 1 && s.selIds.includes(a.id));
  const epoch = useStore((s) => s.thumbEpoch[a.id] || 0);
  const t = makeT(useStore((s) => s.lang));

  const st = () => useStore.getState();
  const selIds = () => st().selIds;

  return (
    <CM.Root>
      <CM.Trigger asChild>
        <div
          style={{ width: w }}
          onClick={(e) => !dragJustEnded() && handleSelectClick(a, e)}
          onDoubleClick={() =>
            !dragJustEnded() && !editing && st().openViewer(a.id)
          }
          onContextMenu={() => {
            if (!selIds().includes(a.id)) st().setSel([a.id], a.id);
          }}
          onMouseDown={(e) => {
            // Finder: pressing an unselected item selects it (so it can be dragged right away)
            if (
              e.button === 0 &&
              !e.metaKey &&
              !e.shiftKey &&
              !selIds().includes(a.id)
            ) {
              st().setSel([a.id], a.id);
            }
            dragStartHandler(() => payloadFor(a))(e);
          }}
          className={`cursor-default overflow-hidden rounded-card border bg-card transition ${
            selected
              ? "border-primary ring-2 ring-primary/25"
              : "border-line hover:border-line-strong hover:shadow-sm"
          }`}
        >
          <div
            className="overflow-hidden border-b border-line bg-side"
            style={{ height: Math.round(w * 0.72) }}
          >
            {/* Keyed remount clears a previous load error when the content or epoch changes */}
            <Thumb
              key={`${a.currentHash}:${epoch}`}
              hash={a.currentHash}
              epoch={epoch}
            />
          </div>
          <div className="px-3 py-2.5">
            {editing ? (
              <RenameInput
                initial={stem(a.fileName)}
                className="-mx-0.5 h-6 w-full rounded border border-primary bg-card px-1.5 text-[13px] font-bold outline-none"
                onCommit={(v) => st().doRename(a.id, v)}
                onCancel={() => st().stopEdit()}
              />
            ) : (
              <div className="truncate text-[13px] font-bold" title={a.title}>
                {a.title}
              </div>
            )}
            <div className="mt-1 flex items-center gap-1.5 text-[10.5px] text-sub">
              <span className="shrink-0 truncate">{timeAgo(a.createdAt)}</span>
              {a.tags.slice(0, 2).map((t) => (
                <span
                  key={t}
                  className="truncate rounded-full border border-line bg-side px-1.5 py-0.5 text-[9.5px] font-bold"
                >
                  #{t}
                </span>
              ))}
              <span className="flex-1" />
              {inbox && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    st().setModal({
                      kind: "move",
                      ids: [a.id],
                      label: a.fileName,
                      fromFolder: a.folder,
                    });
                  }}
                  className="rounded-full bg-primary/10 px-1.5 py-0.5 font-bold text-primary transition hover:bg-primary hover:text-white"
                >
                  {t("archiveTo")}
                </button>
              )}
            </div>
          </div>
        </div>
      </CM.Trigger>
      <CM.Portal>
        <CM.Content className={menuContentCls}>
          {multi ? (
            // Multi-select: act on the whole group
            <>
              <MItem
                icon={<ClipboardCopy className="h-3.5 w-3.5" />}
                label={t("copyN", { n: selIds().length })}
                hint="⌘C"
                onClick={() => st().copyFiles(selIds())}
              />
              <MItem
                icon={<FolderInput className="h-3.5 w-3.5" />}
                label={t("moveNTo", { n: selIds().length })}
                onClick={() =>
                  st().setModal({
                    kind: "move",
                    ids: selIds(),
                    label: t("itemsCount", { n: selIds().length }),
                    fromFolder: null,
                  })
                }
              />
              <MSep />
              <MItem
                danger
                icon={<Trash2 className="h-3.5 w-3.5" />}
                label={t("trashN", { n: selIds().length })}
                hint="⌘⌫"
                onClick={() => st().doTrash(selIds())}
              />
            </>
          ) : (
            <>
              <MItem
                icon={<Eye className="h-3.5 w-3.5" />}
                label={t("open")}
                hint={t("spaceKey")}
                onClick={() => st().openViewer(a.id)}
              />
              <MItem
                icon={<ExternalLink className="h-3.5 w-3.5" />}
                label={t("openInBrowser")}
                onClick={() => api.openInBrowser(a.id).catch(() => {})}
              />
              <MItem
                icon={<FolderOpen className="h-3.5 w-3.5" />}
                label={t("revealInFinder")}
                onClick={() => api.revealAsset(a.id).catch(() => {})}
              />
              <MSep />
              <MItem
                icon={<PencilLine className="h-3.5 w-3.5" />}
                label={t("rename")}
                hint="↵"
                onClick={() => st().startEditAsset(a.id)}
              />
              <MItem
                icon={<TagIcon className="h-3.5 w-3.5" />}
                label={t("tagsMenu")}
                onClick={() => st().setModal({ kind: "tags", asset: a })}
              />
              <MItem
                icon={<ClipboardCopy className="h-3.5 w-3.5" />}
                label={t("copy")}
                hint="⌘C"
                onClick={() => st().copyFiles([a.id])}
              />
              <MItem
                icon={<Copy className="h-3.5 w-3.5" />}
                label={t("duplicate")}
                onClick={() => st().doDuplicateAsset(a.id)}
              />
              <MItem
                icon={<FolderInput className="h-3.5 w-3.5" />}
                label={inbox ? t("archiveTo") : t("moveTo")}
                onClick={() =>
                  st().setModal({
                    kind: "move",
                    ids: [a.id],
                    label: a.fileName,
                    fromFolder: a.folder,
                  })
                }
              />
              <MItem
                icon={<Download className="h-3.5 w-3.5" />}
                label={t("exportCopy")}
                onClick={() => st().doExportAsset(a.id)}
              />
              <MSep />
              <MItem
                danger
                icon={<Trash2 className="h-3.5 w-3.5" />}
                label={t("trash")}
                hint="⌘⌫"
                onClick={() => st().doTrash([a.id])}
              />
            </>
          )}
        </CM.Content>
      </CM.Portal>
    </CM.Root>
  );
}
