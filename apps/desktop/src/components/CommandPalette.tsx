import { Command } from "cmdk";
import { FilePlus2, FolderPlus, RefreshCw, FileCode2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { makeT } from "../lib/i18n";
import { useStore } from "../lib/store";
import type { SearchHit } from "../lib/types";
import { INBOX } from "../lib/types";

export default function CommandPalette() {
  const open = useStore((s) => s.paletteOpen);
  const setPalette = useStore((s) => s.setPalette);
  const t = makeT(useStore((s) => s.lang));
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);

  useEffect(() => {
    if (!open) {
      setQ("");
      setHits([]);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const t = setTimeout(() => {
      if (!q.trim()) {
        setHits([]);
        return;
      }
      api
        .search(q)
        .then(setHits)
        .catch(() => setHits([]));
    }, 130);
    return () => clearTimeout(t);
  }, [q, open]);

  if (!open) return null;

  const st = () => useStore.getState();

  const openAsset = (id: string) => {
    setPalette(false);
    st().openViewer(id);
  };

  return (
    <div
      className="fixed inset-0 z-50 bg-black/25 flex items-start justify-center pt-[12vh]"
      onMouseDown={() => setPalette(false)}
    >
      <div
        className="w-[580px] bg-card rounded-card shadow-2xl border border-line overflow-hidden"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <Command shouldFilter={false} loop>
          <Command.Input
            autoFocus
            value={q}
            onValueChange={setQ}
            placeholder={t("paletteSearchPlaceholder")}
            className="w-full px-4 py-3.5 text-sm outline-none border-b border-line placeholder:text-sub"
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
                className="flex items-center gap-2.5 px-2.5 py-2 rounded-lg cursor-default data-[selected=true]:bg-primary/10"
              >
                <FileCode2 className="w-4 h-4 text-sub shrink-0" />
                <div className="min-w-0 flex-1">
                  <div className="text-[12.5px] font-semibold truncate">{h.asset.title}</div>
                  <div className="text-[10.5px] text-sub truncate">
                    {h.asset.folder === INBOX ? t("inbox") : h.asset.folder || t("libraryRoot")}
                    {h.snippet ? ` · ${h.snippet}` : ""}
                  </div>
                </div>
                <span className="text-[10px] text-sub shrink-0">{t("paletteOpen")}</span>
              </Command.Item>
            ))}

            <div className="px-2.5 pt-2 pb-1 text-[10.5px] font-bold text-sub">{t("commandsGroup")}</div>
            {q.trim() && (
              <Command.Item
                value="cmd-newfolder"
                onSelect={() => {
                  setPalette(false);
                  const parent = st().folder === INBOX ? "" : st().folder;
                  st().doCreateFolder(parent, q.trim());
                }}
                className="flex items-center gap-2.5 px-2.5 py-2 rounded-lg cursor-default data-[selected=true]:bg-primary/10"
              >
                <FolderPlus className="w-4 h-4 text-primary shrink-0" />
                <span className="text-[12.5px]">{t("newFolderQuoted", { name: q.trim() })}</span>
              </Command.Item>
            )}
            <Command.Item
              value="cmd-import"
              onSelect={() => {
                setPalette(false);
                st().pickImport();
              }}
              className="flex items-center gap-2.5 px-2.5 py-2 rounded-lg cursor-default data-[selected=true]:bg-primary/10"
            >
              <FilePlus2 className="w-4 h-4 text-primary shrink-0" />
              <span className="text-[12.5px]">{t("importHtmlCmd")}</span>
              <span className="flex-1" />
              <span className="text-[10px] text-sub">{t("orDragAnywhere")}</span>
            </Command.Item>
            <Command.Item
              value="cmd-rescan"
              onSelect={() => {
                setPalette(false);
                api.rescan().catch(() => {});
              }}
              className="flex items-center gap-2.5 px-2.5 py-2 rounded-lg cursor-default data-[selected=true]:bg-primary/10"
            >
              <RefreshCw className="w-4 h-4 text-primary shrink-0" />
              <span className="text-[12.5px]">{t("rescanLibrary")}</span>
            </Command.Item>
          </Command.List>
          <div className="flex items-center gap-3 px-4 py-2 border-t border-line text-[10px] text-sub">
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
