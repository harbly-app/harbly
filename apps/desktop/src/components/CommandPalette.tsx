import { Command } from "cmdk";
import {
  FilePlus2,
  FileText,
  FolderPlus,
  RefreshCw,
  FileCode2,
  Sparkles,
  SquarePen,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { makeT } from "../lib/i18n";
import { useStore } from "../lib/store";
import type { SearchHit } from "../lib/types";
import { INBOX, isMd } from "../lib/types";

export default function CommandPalette() {
  const open = useStore((s) => s.paletteOpen);
  if (!open) return null;
  // Mounted only while open: q/hits reset naturally on close via unmount
  return <PaletteBody />;
}

function PaletteBody() {
  const setPalette = useStore((s) => s.setPalette);
  const t = makeT(useStore((s) => s.lang));
  const viewerAsset = useStore((s) => s.viewerAsset);
  const selIds = useStore((s) => s.selIds);
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  // The panel is library-scoped; a concrete target (open file or single
  // selection) additionally attaches that file as context
  const aiTarget = viewerAsset?.id ?? (selIds.length === 1 ? selIds[0] : null);

  useEffect(() => {
    const timer = setTimeout(() => {
      if (!q.trim()) {
        setHits([]);
        return;
      }
      api
        .search(q)
        .then(setHits)
        .catch(() => setHits([]));
    }, 130);
    return () => clearTimeout(timer);
  }, [q]);

  const st = () => useStore.getState();

  const openAsset = (id: string) => {
    setPalette(false);
    st().openViewer(id);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/25 pt-[12vh]"
      onMouseDown={() => setPalette(false)}
    >
      <div
        className="w-[580px] overflow-hidden rounded-card border border-line bg-card shadow-2xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <Command shouldFilter={false} loop>
          <Command.Input
            autoFocus
            value={q}
            onValueChange={setQ}
            placeholder={t("paletteSearchPlaceholder")}
            className="w-full border-b border-line px-4 py-3.5 text-sm outline-none placeholder:text-sub"
            onKeyDown={(e) => {
              if (e.key === "Escape") setPalette(false);
            }}
          />
          <Command.List className="max-h-[400px] overflow-y-auto p-2">
            <Command.Empty className="py-8 text-center text-xs text-sub">
              {q.trim() ? t("noMatches") : t("typeToSearch")}
            </Command.Empty>

            {hits.length > 0 && (
              <div className="px-2.5 pt-1.5 pb-1 text-[10.5px] font-bold text-sub">
                {t("assetsGroup")} · {hits.length}
              </div>
            )}
            {hits.map((h) => (
              <Command.Item
                key={h.asset.id}
                value={`asset-${h.asset.id}`}
                onSelect={() => openAsset(h.asset.id)}
                className="flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-2 data-[selected=true]:bg-primary/10"
              >
                {isMd(h.asset.fileName) ? (
                  <FileText className="h-4 w-4 shrink-0 text-sub" />
                ) : (
                  <FileCode2 className="h-4 w-4 shrink-0 text-sub" />
                )}
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[12.5px] font-semibold">
                    {h.asset.title}
                  </div>
                  <div className="truncate text-[10.5px] text-sub">
                    {h.asset.folder === INBOX
                      ? t("inbox")
                      : h.asset.folder || t("libraryRoot")}
                    {h.snippet ? ` · ${h.snippet}` : ""}
                  </div>
                </div>
                <span className="shrink-0 text-[10px] text-sub">
                  {t("paletteOpen")}
                </span>
              </Command.Item>
            ))}

            <div className="px-2.5 pt-2 pb-1 text-[10.5px] font-bold text-sub">
              {t("commandsGroup")}
            </div>
            <Command.Item
              value="cmd-ai"
              onSelect={() => {
                setPalette(false);
                if (aiTarget) st().openAiFor(aiTarget);
                else if (!st().aiOpen) st().toggleAi();
              }}
              className="flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-2 data-[selected=true]:bg-primary/10"
            >
              <Sparkles className="h-4 w-4 shrink-0 text-primary" />
              <span className="text-[12.5px]">{t("aiPanelShow")}</span>
              <span className="flex-1" />
              <kbd className="rounded border border-line bg-side px-1.5 py-0.5 text-[10px] text-sub">
                ⌘J
              </kbd>
            </Command.Item>
            <Command.Item
              value="cmd-newmd"
              onSelect={() => {
                setPalette(false);
                void st().newMarkdown();
              }}
              className="flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-2 data-[selected=true]:bg-primary/10"
            >
              <SquarePen className="h-4 w-4 shrink-0 text-primary" />
              <span className="text-[12.5px]">{t("newMarkdownCmd")}</span>
              <span className="flex-1" />
              <kbd className="rounded border border-line bg-side px-1.5 py-0.5 text-[10px] text-sub">
                ⌘N
              </kbd>
            </Command.Item>
            {q.trim() && (
              <Command.Item
                value="cmd-newfolder"
                onSelect={() => {
                  setPalette(false);
                  const parent = st().folder === INBOX ? "" : st().folder;
                  void st().doCreateFolder(parent, q.trim());
                }}
                className="flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-2 data-[selected=true]:bg-primary/10"
              >
                <FolderPlus className="h-4 w-4 shrink-0 text-primary" />
                <span className="text-[12.5px]">
                  {t("newFolderQuoted", { name: q.trim() })}
                </span>
              </Command.Item>
            )}
            <Command.Item
              value="cmd-import"
              onSelect={() => {
                setPalette(false);
                void st().pickImport();
              }}
              className="flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-2 data-[selected=true]:bg-primary/10"
            >
              <FilePlus2 className="h-4 w-4 shrink-0 text-primary" />
              <span className="text-[12.5px]">{t("importFilesCmd")}</span>
              <span className="flex-1" />
              <span className="text-[10px] text-sub">
                {t("orDragAnywhere")}
              </span>
            </Command.Item>
            <Command.Item
              value="cmd-rescan"
              onSelect={() => {
                setPalette(false);
                api.rescan().catch(() => {});
              }}
              className="flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-2 data-[selected=true]:bg-primary/10"
            >
              <RefreshCw className="h-4 w-4 shrink-0 text-primary" />
              <span className="text-[12.5px]">{t("rescanLibrary")}</span>
            </Command.Item>
          </Command.List>
          <div className="flex items-center gap-3 border-t border-line px-4 py-2 text-[10px] text-sub">
            <span>{t("paletteNav")}</span>
            <span>{t("paletteOpen")}</span>
            <span>{t("paletteClose")}</span>
            <span className="flex-1" />
            <span>{t("paletteFooter")}</span>
          </div>
        </Command>
      </div>
    </div>
  );
}
