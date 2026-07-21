import * as CM from "@radix-ui/react-context-menu";
import {
  ChevronRight,
  ClipboardCopy,
  Copy,
  Download,
  ExternalLink,
  Eye,
  FileCode2,
  FileDown,
  FileText,
  FolderInput,
  FolderOpen,
  FolderPlus,
  Hash,
  Inbox,
  LayoutTemplate,
  PencilLine,
  SquarePen,
  Star,
  Tag as TagIcon,
  Trash2,
} from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { makeT } from "../lib/i18n";
import { dragJustEnded, useStore } from "../lib/store";
import type { TreeFile, TreeNode } from "../lib/types";
import {
  FAVORITES,
  INBOX,
  isHdoc,
  isMd,
  stemName,
  tagView,
} from "../lib/types";
import { dragStartHandler } from "../lib/drag";
import { menuContentCls, MItem, MSep } from "./menu";
import RenameInput from "./RenameInput";

const FILE_LIMIT = 8;

const st = () => useStore.getState();

/// Shared handlers for drag-drop targets
function dropHandlers(rel: string) {
  return {
    onMouseEnter: () => {
      if (st().dragAsset) st().setDropTarget(rel);
    },
    onMouseLeave: () => {
      if (st().dropTarget === rel) st().setDropTarget(null);
    },
  };
}

function gotoFolder(rel: string) {
  if (dragJustEnded()) return;
  st().focusFolder(rel); // navigation + arms Cmd+Backspace folder deletion
}

export default function Sidebar() {
  const tree = useStore((s) => s.tree);
  const inbox = useStore((s) => s.inbox);
  const favCount = useStore((s) => s.favCount);
  const tags = useStore((s) => s.tags);
  const folder = useStore((s) => s.folder);
  const viewerId = useStore((s) => s.viewerAsset?.id ?? null);
  const inboxDrop = useStore((s) => s.dropTarget === INBOX && !!s.dragAsset);
  const rootDrop = useStore((s) => s.dropTarget === "" && !!s.dragAsset);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  // Top-level folders expanded by default: merged during render when the tree
  // changes, so manual collapse/expand choices survive refreshes
  const [prevTree, setPrevTree] = useState<TreeNode | null>(null);
  if (tree !== prevTree) {
    setPrevTree(tree);
    if (tree) {
      const next = new Set(expanded);
      tree.children.forEach((c) => next.add(c.rel));
      setExpanded(next);
    }
  }

  const toggle = (rel: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(rel)) next.delete(rel);
      else next.add(rel);
      return next;
    });

  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const t = makeT(useStore((s) => s.lang));

  return (
    // Collapse/expand animation 0.25s (design token): outer width transitions, inner width stays fixed to avoid content squeeze reflow
    <aside
      className={`flex shrink-0 flex-col overflow-hidden bg-side transition-[width] duration-[250ms] ease-out ${
        sidebarOpen ? "w-[248px] border-r border-line" : "w-0"
      }`}
    >
      <div className="flex h-full w-[248px] shrink-0 flex-col">
        <div className="p-3 pb-1">
          <button
            onClick={() => gotoFolder(INBOX)}
            {...dropHandlers(INBOX)}
            className={`flex w-full items-center gap-2 rounded-ctl px-2.5 py-2 text-[12.5px] transition ${
              inboxDrop
                ? "bg-primary/15 ring-1 ring-primary"
                : folder === INBOX && !viewerId
                  ? "bg-primary/10 font-bold text-primary"
                  : "text-ink hover:bg-card"
            }`}
          >
            <Inbox className="h-4 w-4" />
            <span className="flex-1 text-left">{t("inbox")}</span>
            {inbox > 0 && (
              <span className="grid h-5 min-w-5 place-items-center rounded-full bg-primary px-1.5 text-[10.5px] font-bold text-white">
                {inbox}
              </span>
            )}
          </button>
          <button
            onClick={() => gotoFolder(FAVORITES)}
            className={`flex w-full items-center gap-2 rounded-ctl px-2.5 py-2 text-[12.5px] transition ${
              folder === FAVORITES && !viewerId
                ? "bg-primary/10 font-bold text-primary"
                : "text-ink hover:bg-card"
            }`}
          >
            <Star className="h-4 w-4" />
            <span className="flex-1 text-left">{t("favorites")}</span>
            {favCount > 0 && (
              <span className="text-[10.5px] text-sub">{favCount}</span>
            )}
          </button>
        </div>

        <div className="flex items-center justify-between px-5 pt-3 pb-1">
          <span className="text-[11px] font-bold tracking-wide text-sub">
            {t("foldersSection")}
          </span>
          <button
            onClick={() => st().setModal({ kind: "newFolder", parent: "" })}
            title={t("newFolder")}
            className="grid h-5 w-5 place-items-center rounded text-sub transition hover:bg-card hover:text-primary"
          >
            <FolderPlus className="h-3.5 w-3.5" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto px-3 pb-2">
          <button
            onClick={() => gotoFolder("")}
            {...dropHandlers("")}
            className={`flex w-full items-center gap-1.5 rounded-ctl px-2.5 py-1.5 text-[12.5px] transition ${
              rootDrop
                ? "bg-primary/15 ring-1 ring-primary"
                : folder === "" && !viewerId
                  ? "bg-primary/10 font-bold text-primary"
                  : "hover:bg-card"
            }`}
            title={t("dropToRoot")}
          >
            <span className="w-4" />
            <span className="flex-1 truncate text-left">{t("allAssets")}</span>
            {tree && (
              <span className="text-[10.5px] text-sub">{tree.count}</span>
            )}
          </button>

          {tree?.children.map((c) => (
            <TreeRow
              key={c.rel}
              node={c}
              depth={0}
              folder={folder}
              viewerId={viewerId}
              expanded={expanded}
              toggle={toggle}
            />
          ))}

          {/* Files directly under the library root */}
          {tree?.files.map((f) => (
            <FileRow
              key={f.id}
              f={f}
              folderRel=""
              depth={0}
              viewerId={viewerId}
            />
          ))}

          {tags.length > 0 && (
            <>
              <div className="px-2.5 pt-4 pb-1 text-[11px] font-bold tracking-wide text-sub">
                {t("tagsSection")}
              </div>
              {tags.map((t) => {
                const active = folder === tagView(t.name) && !viewerId;
                return (
                  <button
                    key={t.name}
                    onClick={() => gotoFolder(tagView(t.name))}
                    className={`flex w-full items-center gap-1.5 rounded-ctl px-2.5 py-1.5 text-[12.5px] transition ${
                      active
                        ? "bg-primary/10 font-bold text-primary"
                        : "hover:bg-card"
                    }`}
                  >
                    <Hash
                      className={`h-3.5 w-3.5 ${active ? "text-primary" : "text-sub"}`}
                    />
                    <span className="flex-1 truncate text-left">{t.name}</span>
                    <span
                      className={`text-[10.5px] ${active ? "text-primary" : "text-sub"}`}
                    >
                      {t.count}
                    </span>
                  </button>
                );
              })}
            </>
          )}
        </div>
      </div>
    </aside>
  );
}

function TreeRow(props: {
  node: TreeNode;
  depth: number;
  folder: string;
  viewerId: string | null;
  expanded: Set<string>;
  toggle: (rel: string) => void;
}) {
  const { node, depth, folder, viewerId, expanded, toggle } = props;
  const isDrop = useStore((s) => s.dropTarget === node.rel && !!s.dragAsset);
  const editing = useStore((s) => s.editingFolder === node.rel);
  const t = makeT(useStore((s) => s.lang));
  const open = expanded.has(node.rel);
  const active = folder === node.rel && !viewerId;
  const expandable = node.children.length > 0 || node.files.length > 0;
  const extra = node.files.length - FILE_LIMIT;

  return (
    // On drag hover, highlight the whole folder block (title + expanded area) as one container rather than tinting row by row
    <div
      className={`rounded-ctl transition-colors ${isDrop ? "bg-primary/10 ring-1 ring-primary/60" : ""}`}
    >
      <CM.Root>
        <CM.Trigger asChild>
          <div
            className={`flex cursor-default items-center gap-0.5 rounded-ctl py-1.5 pr-2.5 text-[12.5px] transition ${
              active && !isDrop
                ? "bg-primary/10 font-bold text-primary"
                : isDrop
                  ? ""
                  : "hover:bg-card"
            }`}
            style={{ paddingLeft: 6 + depth * 13 }}
            onClick={() => !editing && gotoFolder(node.rel)}
            {...dropHandlers(node.rel)}
          >
            <button
              onClick={(e) => {
                e.stopPropagation();
                if (expandable) toggle(node.rel);
              }}
              className={`grid h-4 w-4 shrink-0 place-items-center rounded text-sub ${
                expandable ? "hover:text-ink" : "opacity-0"
              }`}
            >
              <ChevronRight
                className={`h-3 w-3 transition-transform ${open ? "rotate-90" : ""}`}
              />
            </button>
            {editing ? (
              <RenameInput
                initial={node.name}
                onCommit={(v) => st().doRenameFolder(node.rel, v)}
                onCancel={() => st().stopEdit()}
              />
            ) : (
              <>
                <span className="flex-1 truncate text-left">{node.name}</span>
                <span
                  className={`text-[10.5px] ${active ? "text-primary" : "text-sub"}`}
                >
                  {node.count}
                </span>
              </>
            )}
          </div>
        </CM.Trigger>
        <CM.Portal>
          <CM.Content className={menuContentCls}>
            <MItem
              icon={<SquarePen className="h-3.5 w-3.5" />}
              label={t("newMarkdownCmd")}
              onClick={() => st().newMarkdown(node.rel)}
            />
            <MItem
              icon={<LayoutTemplate className="h-3.5 w-3.5" />}
              label={t("newHdocCmd")}
              onClick={() => st().newHdoc(node.rel)}
            />
            <MItem
              icon={<FolderPlus className="h-3.5 w-3.5" />}
              label={t("newSubfolder")}
              onClick={() =>
                st().setModal({ kind: "newFolder", parent: node.rel })
              }
            />
            <MItem
              icon={<FolderOpen className="h-3.5 w-3.5" />}
              label={t("openFolderInFinder")}
              onClick={() => api.revealFolder(node.rel).catch(() => {})}
            />
            <MSep />
            <MItem
              icon={<PencilLine className="h-3.5 w-3.5" />}
              label={t("rename")}
              onClick={() => st().startEditFolder(node.rel)}
            />
            <MItem
              icon={<Copy className="h-3.5 w-3.5" />}
              label={t("duplicate")}
              onClick={() => st().doDuplicateFolder(node.rel)}
            />
            <MItem
              icon={<Download className="h-3.5 w-3.5" />}
              label={t("exportZip")}
              onClick={() => st().doExportFolder(node.rel)}
            />
            <MSep />
            {/* Empty folders trash instantly (Cmd+Z undoable); non-empty ones confirm first */}
            <MItem
              danger
              icon={<Trash2 className="h-3.5 w-3.5" />}
              label={t("trash")}
              onClick={() => st().requestDeleteFolder(node.rel)}
            />
          </CM.Content>
        </CM.Portal>
      </CM.Root>

      {open && (
        <>
          {node.children.map((c) => (
            <TreeRow {...props} key={c.rel} node={c} depth={depth + 1} />
          ))}
          {node.files.slice(0, FILE_LIMIT).map((f) => (
            <FileRow
              key={f.id}
              f={f}
              folderRel={node.rel}
              depth={depth + 1}
              viewerId={viewerId}
            />
          ))}
          {extra > 0 && (
            <button
              onClick={() => gotoFolder(node.rel)}
              {...dropHandlers(node.rel)}
              className="w-full rounded-ctl py-1 text-left text-[11.5px] text-sub transition hover:text-primary"
              style={{ paddingLeft: 6 + (depth + 1) * 13 + 20 }}
            >
              {t("moreN", { n: extra })}
            </button>
          )}
        </>
      )}
    </div>
  );
}

function FileRow(props: {
  f: TreeFile;
  folderRel: string;
  depth: number;
  viewerId: string | null;
}) {
  const { f, folderRel, depth, viewerId } = props;
  const active = viewerId === f.id;
  const editing = useStore((s) => s.editingAsset === f.id);
  const t = makeT(useStore((s) => s.lang));
  const rel = folderRel ? `${folderRel}/${f.name}` : f.name;
  const FileIcon = isMd(f.name)
    ? FileText
    : isHdoc(f.name)
      ? LayoutTemplate
      : FileCode2;

  const openIt = () => {
    if (dragJustEnded()) return;
    if (st().folder !== folderRel) st().setFolder(folderRel);
    st().openViewer(f.id);
  };

  const withMeta =
    (fn: (a: import("../lib/types").AssetMeta) => void) => () => {
      api
        .assetGet(f.id)
        .then(fn)
        .catch(() => {});
    };

  return (
    <CM.Root>
      <CM.Trigger asChild>
        <div
          onClick={() => !editing && openIt()}
          onMouseDown={dragStartHandler({
            ids: [f.id],
            rels: [rel],
            label: f.name,
            fromFolder: folderRel,
          })}
          {...dropHandlers(folderRel)}
          className={`flex cursor-default items-center gap-1.5 rounded-ctl py-[5px] pr-2.5 text-[12px] transition ${
            active
              ? "bg-primary/10 font-bold text-primary"
              : "text-sub2 hover:bg-card"
          }`}
          style={{ paddingLeft: 6 + depth * 13 + 20 }}
        >
          <FileIcon
            className={`h-3.5 w-3.5 shrink-0 ${active ? "text-primary" : "text-sub"}`}
          />
          {editing ? (
            <RenameInput
              initial={stemName(f.name)}
              onCommit={(v) => st().doRename(f.id, v)}
              onCancel={() => st().stopEdit()}
            />
          ) : (
            <span className="flex-1 truncate">{f.name}</span>
          )}
          {f.favorite && !editing && (
            <Star className="h-3 w-3 shrink-0 fill-warn text-warn" />
          )}
        </div>
      </CM.Trigger>
      <CM.Portal>
        <CM.Content className={menuContentCls}>
          <MItem
            icon={<Eye className="h-3.5 w-3.5" />}
            label={t("open")}
            onClick={openIt}
          />
          <MItem
            icon={<ExternalLink className="h-3.5 w-3.5" />}
            label={
              isHdoc(f.name)
                ? t("previewInBrowser")
                : isMd(f.name)
                  ? t("openWithDefaultApp")
                  : t("openInBrowser")
            }
            onClick={() =>
              // .hdoc has no default app — bake and open in the browser, same as the grid
              isHdoc(f.name)
                ? api.previewHdoc(f.id).catch(() => {})
                : api.openInBrowser(f.id).catch(() => {})
            }
          />
          <MItem
            icon={<FolderOpen className="h-3.5 w-3.5" />}
            label={t("revealInFinder")}
            onClick={() => api.revealAsset(f.id).catch(() => {})}
          />
          <MSep />
          <MItem
            icon={<PencilLine className="h-3.5 w-3.5" />}
            label={t("rename")}
            onClick={() => st().startEditAsset(f.id)}
          />
          <MItem
            icon={
              <Star
                className={`h-3.5 w-3.5 ${f.favorite ? "fill-warn text-warn" : ""}`}
              />
            }
            label={f.favorite ? t("removeFavorite") : t("addFavorite")}
            onClick={() => api.setFavorite(f.id, !f.favorite).catch(() => {})}
          />
          <MItem
            icon={<TagIcon className="h-3.5 w-3.5" />}
            label={t("tagsMenu")}
            onClick={withMeta((a) =>
              st().setModal({ kind: "tags", assets: [a] }),
            )}
          />
          <MItem
            icon={<ClipboardCopy className="h-3.5 w-3.5" />}
            label={t("copy")}
            onClick={() => st().copyFiles([f.id])}
          />
          <MItem
            icon={<Copy className="h-3.5 w-3.5" />}
            label={t("duplicate")}
            onClick={() => st().doDuplicateAsset(f.id)}
          />
          <MItem
            icon={<FolderInput className="h-3.5 w-3.5" />}
            label={t("moveTo")}
            onClick={() =>
              st().setModal({
                kind: "move",
                ids: [f.id],
                label: f.name,
                fromFolder: folderRel,
              })
            }
          />
          <MItem
            icon={<Download className="h-3.5 w-3.5" />}
            label={t("exportCopy")}
            onClick={() => st().doExportAsset(f.id)}
          />
          {isHdoc(f.name) && (
            <MItem
              icon={<FileDown className="h-3.5 w-3.5" />}
              label={t("exportHdocCmd")}
              onClick={() => st().doExportHdoc(f.id)}
            />
          )}
          <MSep />
          <MItem
            danger
            icon={<Trash2 className="h-3.5 w-3.5" />}
            label={t("trash")}
            onClick={() => st().doTrash([f.id])}
          />
        </CM.Content>
      </CM.Portal>
    </CM.Root>
  );
}
