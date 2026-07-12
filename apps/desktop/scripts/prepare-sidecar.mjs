// Build harbly-mcp and stage it where Tauri's externalBin contract expects it:
// src-tauri/binaries/harbly-mcp-<target-triple>. From there tauri-build copies
// it next to the app binary for `tauri dev`, and the bundler ships it inside
// the .app (Contents/MacOS on macOS) for `tauri build` — which is exactly
// where mcp_server_path() in ai.rs resolves it at runtime.
//
// Runs as the first step of both beforeDevCommand and beforeBuildCommand;
// once externalBin is configured, compiling the desktop app at all requires
// the staged file to exist.
import { execFileSync, execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = dirname(dirname(fileURLToPath(import.meta.url))); // apps/desktop
const binDir = join(appDir, "src-tauri", "binaries");
const ext = process.platform === "win32" ? ".exe" : "";

const run = (cmd, args) =>
  execFileSync(cmd, args, { stdio: "inherit", cwd: appDir });
// cargo metadata resolves the workspace target dir wherever it lives
// (CARGO_TARGET_DIR and .cargo/config.toml overrides included)
const targetDir = JSON.parse(
  execSync("cargo metadata --no-deps --format-version 1", { cwd: appDir }),
).target_directory;

const host = /host: (\S+)/.exec(execSync("rustc -vV").toString())[1];
// Tauri exports the triple it is building for; plain `pnpm dev` has only the host
const triple = process.env.TAURI_ENV_TARGET_TRIPLE ?? host;

mkdirSync(binDir, { recursive: true });
const dest = join(binDir, `harbly-mcp-${triple}${ext}`);

if (triple === "universal-apple-darwin") {
  // Not a rustc target: build each slice, then fuse them
  const slices = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const t of slices)
    run("cargo", ["build", "-p", "harbly-mcp", "--release", "--target", t]);
  run("lipo", [
    "-create",
    "-output",
    dest,
    ...slices.map((t) => join(targetDir, t, "release", "harbly-mcp")),
  ]);
} else {
  // Native builds skip --target so harbly-mcp shares the plain release dir
  // (and its build cache) with the app itself
  const cross = triple !== host;
  run("cargo", [
    "build",
    "-p",
    "harbly-mcp",
    "--release",
    ...(cross ? ["--target", triple] : []),
  ]);
  copyFileSync(
    join(targetDir, ...(cross ? [triple] : []), "release", `harbly-mcp${ext}`),
    dest,
  );
}
console.log(`sidecar staged: ${dest}`);
