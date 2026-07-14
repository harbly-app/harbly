// Generate THIRD-PARTY-NOTICES.md covering every third-party dependency the
// shipped app carries: Rust crates linked into the binary (enumerated from the
// resolved Cargo graph) and npm packages Vite bundles into the frontend. Each
// dependency's own license text is reproduced where it ships one, so the
// distributed .app satisfies the attribution clauses of permissive licenses
// (MIT/BSD/ISC/…) alongside Harbly's own AGPL-3.0-only terms.
//
// Runs as a step of beforeDevCommand and beforeBuildCommand; the output is
// git-ignored and regenerated from the lockfiles on every build, so it can
// never drift from the actual dependency set. Tauri ships it as a bundled
// resource (see tauri.conf.json → bundle.resources).
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = dirname(dirname(fileURLToPath(import.meta.url))); // apps/desktop
const repoRoot = dirname(dirname(appDir)); // workspace root
const outDir = join(appDir, "src-tauri", "resources");
const outFile = join(outDir, "THIRD-PARTY-NOTICES.md");

const LICENSE_FILE_RE =
  /^(LICEN[SC]E|COPYING|COPYRIGHT|NOTICE|UNLICENSE)(\.|-|$)/i;
const MAX_TEXT = 64 * 1024; // guard against a pathological license file
const BUF = 128 * 1024 * 1024; // cargo/pnpm JSON can be large

// Collect the license/notice files a package ships in its source directory.
function readLicenseTexts(dir) {
  let names;
  try {
    names = readdirSync(dir);
  } catch {
    return [];
  }
  const out = [];
  for (const name of names.sort()) {
    if (!LICENSE_FILE_RE.test(name)) continue;
    const p = join(dir, name);
    try {
      if (!statSync(p).isFile()) continue;
      out.push({ name, text: readFileSync(p, "utf8").slice(0, MAX_TEXT) });
    } catch {
      /* unreadable — skip */
    }
  }
  return out;
}

// Rust crates that actually reach the distributed artifact: the non-dev
// dependency closure of the two shipped binaries — the desktop app and the
// bundled MCP sidecar. `cargo metadata` alone lists dev-dependencies of every
// transitive crate too, which never ship; walking the resolve graph and
// dropping dev-only edges keeps attribution both accurate and lean.
function rustEntries() {
  const meta = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
      cwd: appDir,
      maxBuffer: BUF,
    }),
  );
  const workspace = new Set(meta.workspace_members);
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));

  const queue = meta.workspace_members.filter((id) => {
    const name = byId.get(id)?.name;
    return name === "harbly-app" || name === "harbly-mcp";
  });
  const reached = new Set();
  while (queue.length) {
    const node = nodes.get(queue.pop());
    if (!node) continue;
    for (const dep of node.deps) {
      // A `null` kind is a normal (runtime) edge; "build" edges pull in
      // proc-macros/build scripts. Only "dev" edges are excluded.
      const ships = (dep.dep_kinds || []).some((k) => k.kind !== "dev");
      if (!ships || reached.has(dep.pkg)) continue;
      reached.add(dep.pkg);
      queue.push(dep.pkg);
    }
  }

  const entries = [];
  for (const id of reached) {
    const pkg = byId.get(id);
    if (!pkg || !pkg.source || workspace.has(id)) continue;
    entries.push({
      name: pkg.name,
      version: pkg.version,
      license: pkg.license || pkg.license_file || "(unspecified)",
      source: pkg.repository || "",
      texts: readLicenseTexts(dirname(pkg.manifest_path)),
    });
  }
  return dedupeSort(entries);
}

// npm packages in the desktop app's production closure (what Vite bundles).
// `pnpm licenses list` groups by license id: { "MIT": [ {name, versions, paths} ] }.
function npmEntries() {
  let raw;
  try {
    raw = execFileSync(
      "pnpm",
      ["--filter", "@harbly/desktop", "licenses", "list", "--prod", "--json"],
      { cwd: repoRoot, maxBuffer: BUF },
    );
  } catch (e) {
    raw = e.stdout; // pnpm can exit non-zero while still emitting valid JSON
  }
  if (!raw || !raw.length) return [];
  let data;
  try {
    data = JSON.parse(raw);
  } catch {
    return [];
  }
  const entries = [];
  for (const [license, pkgs] of Object.entries(data)) {
    for (const pkg of pkgs) {
      const versions = pkg.versions?.length
        ? pkg.versions
        : [pkg.version].filter(Boolean);
      const paths = pkg.paths?.length ? pkg.paths : [pkg.path].filter(Boolean);
      entries.push({
        name: pkg.name,
        version: versions.join(", "),
        license: pkg.license || license || "(unspecified)",
        source: pkg.homepage || "",
        texts: paths.flatMap(readLicenseTexts),
      });
    }
  }
  return dedupeSort(entries);
}

function dedupeSort(entries) {
  const seen = new Map();
  for (const e of entries) seen.set(`${e.name}@${e.version}`, e);
  return [...seen.values()].sort(
    (a, b) =>
      a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
  );
}

function renderSection(title, entries) {
  const lines = [`## ${title} (${entries.length})`, ""];
  for (const e of entries) {
    lines.push(`### ${e.name} ${e.version}`, "");
    lines.push(`- License: ${e.license}`);
    if (e.source) lines.push(`- Source: ${e.source}`);
    lines.push("");
    for (const t of e.texts) {
      lines.push("```text", t.text.trimEnd(), "```", "");
    }
  }
  return lines.join("\n");
}

const rust = rustEntries();
const npm = npmEntries();

const header = [
  "# Third-Party Notices",
  "",
  "Harbly is distributed under AGPL-3.0-only. The components below are bundled",
  "with the app under their own open-source licenses, reproduced here in full",
  "where provided. This file is generated from the Cargo and pnpm lockfiles by",
  "`apps/desktop/scripts/generate-notices.mjs` — do not edit by hand.",
  "",
].join("\n");

mkdirSync(outDir, { recursive: true });
writeFileSync(
  outFile,
  `${[
    header,
    renderSection("Rust crates", rust),
    renderSection("npm packages", npm),
  ].join("\n")}\n`,
);
console.log(
  `third-party notices: ${rust.length} crates + ${npm.length} npm packages -> ${outFile}`,
);
