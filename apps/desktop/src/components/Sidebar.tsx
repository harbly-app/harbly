import * as CM from "@radix-ui/react-context-menu";
import {
  ChevronRight,
  ClipboardCopy,
  Copy,
  Download,
  ExternalLink,
  Eye,
  FileCode2,
  FolderInput,
  FolderOpen,
  FolderPlus,
  Hash,
  Inbox,
  PencilLine,
  Tag as TagIcon,
  Trash2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { makeT } from "../lib/i18n";
import { dragJustEnded, useStore } from "../lib/store";
import type { TreeFile, TreeNode } from "../lib/types";
import { INBOX } from "../lib/types";
import { dragStartHandler, menuContentCls, MItem, MSep } from "./menu";
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
  st().closeViewer();
  st().setFolder(rel);
}

export default function Sidebar() {
  const tree = useStore((s) => s.tree);
  const inbox = useStore((s) => s.inbox);
  const tags = useStore((s) => s.tags);
  const folder = useStore((s) => s.folder);
  const viewerId = useStore((s) => s.viewerAsset?.id ?? null);
  const inboxDrop = useStore((s) => s.dropTarget === INBOX && !!s.dragAsset);
  const rootDrop = useStore((s) => s.dropTarget === "" && !!s.dragAsset);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  // Top-level folders expanded by default
  useEffect(() => {
    if (!tree) return;
    setExpanded((prev) => {
      const next = new Set(prev);
      tree.children.forEach((c) => next.add(c.rel));
      return next;
    });
  }, [tree]);

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
      className={`shrink-0 bg-side flex flex-col overflow-hidden transition-[width] duration-[250ms] ease-out ${
        sidebarOpen ? "w-[248px] border-r border-line" : "w-0"
      }`}
    >
      <div className="w-[248px] shrink-0 flex flex-col h-full">
      <div className="p-3 pb-1">
        <button
          onClick={() => gotoFolder(INBOX)}
          {...dropHandlers(INBOX)}
          className={`w-full flex items-center gap-2 px-2.5 py-2 rounded-ctl text-[12.5px] transition ${
            inboxDrop
              ? "bg-primary/15 ring-1 ring-primary"
              : folder === INBOX && !viewerId
                ? "bg-primary/10 text-primary font-bold"
                : "hover:bg-white text-ink"
          }`}
        >
          <Inbox className="w-4 h-4" />
          <span className="flex-1 text-left">{t("inbox")}</span>
          {inbox > 0 && (
            <span className="min-w-5 h-5 px-1.5 grid place-items-center rounded-full bg-primary text-white text-[10.5px] font-bold">
              {inbox}
            </span>
          )}
        </button>
      </div>

      <div className="flex items-center justify-between px-5 pt-3 pb-1">
        <span className="text-[11px] font-bold text-sub tracking-wide">{t("foldersSection")}</span>
        <button
          onClick={() => st().setModal({ kind: "newFolder", parent: "" })}
          title={t("newFolder")}
          className="w-5 h-5 grid place-items-center rounded text-sub hover:text-primary hover:bg-white transition"
        >
          <FolderPlus className="w-3.5 h-3.5" />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto px-3 pb-2">
        <button
          onClick={() => gotoFolder("")}
          {...dropHandlers("")}
          className={`w-full flex items-center gap-1.5 px-2.5 py-1.5 rounded-ctl text-[12.5px] transition ${
            rootDrop
              ? "bg-primary/15 ring-1 ring-primary"
              : folder === "" && !viewerId
                ? "bg-primary/10 text-primary font-bold"
                : "hover:bg-white"
          }`}
          title={t("dropToRoot")}
        >
          <span className="w-4" />
          <span className="flex-1 text-left truncate">{t("allAssets")}</span>
          {tree && <span className="text-[10.5px] text-sub">{tree.count}</span>}
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
          <FileRow key={f.id} f={f} folderRel="" depth={0} viewerId={viewerId} />
        ))}

        {tags.length > 0 && (
          <>
            <div className="px-2.5 pt-4 pb-1 text-[11px] font-bold text-sub tracking-wide">{t("tagsSection")}</div>
            {tags.map((t) => {
              const active = folder === `#${t.name}` && !viewerId;
              return (
                <button
                  key={t.name}
                  onClick={() => gotoFolder(`#${t.name}`)}
                  className={`w-full flex items-center gap-1.5 px-2.5 py-1.5 rounded-ctl text-[12.5px] transition ${
                    active ? "bg-primary/10 text-primary font-bold" : "hover:bg-white"
                  }`}
                >
                  <Hash className={`w-3.5 h-3.5 ${active ? "text-primary" : "text-sub"}`} />
                  <span className="flex-1 text-left truncate">{t.name}</span>
                  <span className={`text-[10.5px] ${active ? "text-primary" : "text-sub"}`}>{t.count}</span>
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
    <div className={`rounded-ctl transition-colors ${isDrop ? "bg-primary/10 ring-1 ring-primary/60" : ""}`}>
      <CM.Root>
        <CM.Trigger asChild>
          <div
            className={`flex items-center gap-0.5 pr-2.5 py-1.5 rounded-ctl text-[12.5px] cursor-default transition ${
              active && !isDrop ? "bg-primary/10 text-primary font-bold" : isDrop ? "" : "hover:bg-white"
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
              className={`w-4 h-4 shrink-0 grid place-items-center rounded text-sub ${
                expandable ? "hover:text-ink" : "opacity-0"
              }`}
            >
              <ChevronRight className={`w-3 h-3 transition-transform ${open ? "rotate-90" : ""}`} />
            </button>
            {editing ? (
              <RenameInput
                initial={node.name}
                onCommit={(v) => st().doRenameFolder(node.rel, v)}
                onCancel={() => st().stopEdit()}
              />
            ) : (
              <>
                <span className="flex-1 text-left truncate">{node.name}</span>
                <span className={`text-[10.5px] ${active ? "text-primary" : "text-sub"}`}>{node.count}</span>
              </>
            )}
          </div>
        </CM.Trigger>
        <CM.Portal>
          <CM.Content className={menuContentCls}>
            <MItem
              icon={<FolderPlus className="w-3.5 h-3.5" />}
              label={t("newSubfolder")}
              onClick={() => st().setModal({ kind: "newFolder", parent: node.rel })}
            />
            <MItem
              icon={<FolderOpen className="w-3.5 h-3.5" />}
              label={t("openFolderInFinder")}
              onClick={() => api.revealFolder(node.rel).catch(() => {})}
            />
            <MSep />
            <MItem
              icon={<PencilLine className="w-3.5 h-3.5" />}
              label={t("rename")}
              onClick={() => st().startEditFolder(node.rel)}
            />
            <MItem
              icon={<Copy className="w-3.5 h-3.5" />}
              label={t("duplicate")}
              onClick={() => st().doDuplicateFolder(node.rel)}
            />
            <MItem
              icon={<Download className="w-3.5 h-3.5" />}
              label={t("exportZip")}
              onClick={() => st().doExportFolder(node.rel)}
            />
            <MSep />
            {/* Undoable via Cmd+Z, no confirmation dialog needed (Finder semantics) */}
            <MItem
              danger
              icon={<Trash2 className="w-3.5 h-3.5" />}
              label={t("trash")}
              onClick={() => st().doDeleteFolder(node.rel)}
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
            <FileRow key={f.id} f={f} folderRel={node.rel} depth={depth + 1} viewerId={viewerId} />
          ))}
          {extra > 0 && (
            <button
              onClick={() => gotoFolder(node.rel)}
              {...dropHandlers(node.rel)}
              className="w-full text-left text-[11.5px] text-sub hover:text-primary py-1 rounded-ctl transition"
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

function FileRow(props: { f: TreeFile; folderRel: string; depth: number; viewerId: string | null }) {
  const { f, folderRel, depth, viewerId } = props;
  const active = viewerId === f.id;
  const editing = useStore((s) => s.editingAsset === f.id);
  const t = makeT(useStore((s) => s.lang));
  const rel = folderRel ? `${folderRel}/${f.name}` : f.name;

  const openIt = () => {
    if (dragJustEnded()) return;
    if (st().folder !== folderRel) st().setFolder(folderRel);
    st().openViewer(f.id);
  };

  const withMeta = (fn: (a: import("../lib/types").AssetMeta) => void) => () => {
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
          onMouseDown={dragStartHandler({ ids: [f.id], rels: [rel], label: f.name, fromFolder: folderRel })}
          {...dropHandlers(folderRel)}
          className={`flex items-center gap-1.5 pr-2.5 py-[5px] rounded-ctl text-[12px] cursor-default transition ${
            active ? "bg-primary/10 text-primary font-bold" : "text-sub2 hover:bg-white"
          }`}
          style={{ paddingLeft: 6 + depth * 13 + 20 }}
        >
          <FileCode2 className={`w-3.5 h-3.5 shrink-0 ${active ? "text-primary" : "text-sub"}`} />
          {editing ? (
            <RenameInput
              initial={f.name.replace(/\.(html?|htm)$/i, "")}
              onCommit={(v) => st().doRename(f.id, v)}
              onCancel={() => st().stopEdit()}
            />
          ) : (
            <span className="flex-1 truncate">{f.name}</span>
          )}
        </div>
      </CM.Trigger>
      <CM.Portal>
        <CM.Content className={menuContentCls}>
          <MItem icon={<Eye className="w-3.5 h-3.5" />} label={t("open")} onClick={openIt} />
          <MItem
            icon={<ExternalLink className="w-3.5 h-3.5" />}
            label={t("openInBrowser")}
            onClick={() => api.openInBrowser(f.id).catch(() => {})}
          />
          <MItem
            icon={<FolderOpen className="w-3.5 h-3.5" />}
            label={t("revealInFinder")}
            onClick={() => api.revealAsset(f.id).catch(() => {})}
          />
          <MSep />
          <MItem
            icon={<PencilLine className="w-3.5 h-3.5" />}
            label={t("rename")}
            onClick={() => st().startEditAsset(f.id)}
          />
          <MItem
            icon={<TagIcon className="w-3.5 h-3.5" />}
            label={t("tagsMenu")}
            onClick={withMeta((a) => st().setModal({ kind: "tags", asset: a }))}
          />
          <MItem
            icon={<ClipboardCopy className="w-3.5 h-3.5" />}
            label={t("copy")}
            onClick={() => st().copyFiles([f.id])}
          />
          <MItem
            icon={<Copy className="w-3.5 h-3.5" />}
            label={t("duplicate")}
            onClick={() => st().doDuplicateAsset(f.id)}
          />
          <MItem
            icon={<FolderInput className="w-3.5 h-3.5" />}
            label={t("moveTo")}
            onClick={() => st().setModal({ kind: "move", ids: [f.id], label: f.name, fromFolder: folderRel })}
          />
          <MItem
            icon={<Download className="w-3.5 h-3.5" />}
            label={t("exportCopy")}
            onClick={() => st().doExportAsset(f.id)}
          />
          <MSep />
          <MItem
            danger
            icon={<Trash2 className="w-3.5 h-3.5" />}
            label={t("trash")}
            onClick={() => st().doTrash([f.id])}
          />
        </CM.Content>
      </CM.Portal>
    </CM.Root>
  );
}
