/**
 * Browser-only dev harness for the hdoc editor (open /dev-hdoc.html under
 * `vite dev`). Tauri IPC is replaced by an in-memory file via the official
 * mock, so the full editor — autosave chain, checkpoint, conflict detection —
 * runs against real code paths without the desktop shell. Test hooks are
 * exposed on `window.__hdoc` for driving/inspection from the console.
 * Never bundled into the app (only index.html is a build input).
 */
import { mockIPC } from "@tauri-apps/api/mocks";
import { createRoot } from "react-dom/client";
import HdocEditor from "../components/HdocEditor";
import { useStore } from "../lib/store";
import type { AssetMeta } from "../lib/types";
import "../styles.css";

const disk = {
  text: `<h-doc v="1" theme="paper">\n  <h1></h1>\n  <p></p>\n</h-doc>\n`,
};
const writes: string[] = [];
let rev = 1;

const meta = (): AssetMeta => ({
  id: "dev-asset",
  relPath: "dev.hdoc",
  fileName: "dev.hdoc",
  folder: "",
  title: "dev",
  source: "create",
  sizeBytes: disk.text.length,
  currentHash: `h${rev}`,
  verCount: 1,
  createdAt: 0,
  updatedAt: 0,
  tags: [],
  favorite: false,
});

// Installed before render, so every invoke() from the editor hits the mock.
mockIPC((cmd, payload) => {
  switch (cmd) {
    case "asset_read_text":
      return disk.text;
    case "asset_write": {
      disk.text = (payload as { content: string }).content;
      writes.push(disk.text);
      rev++;
      return meta();
    }
    case "asset_checkpoint":
      return true;
    default:
      // theme sync, menus, … — irrelevant in the harness
      return null;
  }
});

const asset = meta();
useStore.setState({ phase: "main", viewerAsset: asset });

declare global {
  interface Window {
    __hdoc: {
      writes: string[];
      disk: { text: string };
      store: typeof useStore;
      /** Simulate an external on-disk change (conflict banner testing). */
      externalEdit: (text: string) => void;
    };
  }
}
window.__hdoc = {
  writes,
  disk,
  store: useStore,
  externalEdit: (text: string) => {
    disk.text = text;
    rev++;
    useStore.setState({ viewerAsset: meta() });
  },
};

const rootEl = document.getElementById("root");
if (rootEl) createRoot(rootEl).render(<HdocEditor asset={asset} />);
