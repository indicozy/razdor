// Reads the release notes (changelog/X.Y.Z.md) at build time for the changes pages, the downloads and «Что нового».
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

export interface ShaLine {
  hash: string;
  file: string;
}

export interface Release {
  version: string;
  date: string;
  added: string[];
  changed: string[];
  fixed: string[];
  /** The SHA-256 block of the release (empty for a version that has none). */
  sha256: ShaLine[];
}

const GROUPS: Record<string, keyof Omit<Release, "version" | "date" | "sha256">> = {
  "Добавлено": "added",
  "Изменено": "changed",
  "Исправлено": "fixed",
};

function changelogDir(): string | null {
  const candidates = [
    fileURLToPath(new URL("../../../changelog/", import.meta.url)),
    resolve(process.cwd(), "../changelog"),
  ];
  return candidates.find((d) => existsSync(d)) ?? null;
}

export function parseRelease(text: string): Release | null {
  const head = text.match(/^## (\d+\.\d+\.\d+) — (\d{4}-\d{2}-\d{2})\s*$/m);
  if (!head) return null;
  const rel: Release = { version: head[1], date: head[2], added: [], changed: [], fixed: [], sha256: [] };
  let group: keyof Omit<Release, "version" | "date" | "sha256"> | null = null;
  let headings = 0;
  for (const line of text.split("\n")) {
    if (line.startsWith("## ")) {
      // only the first version's block: stop at the second version heading
      if (++headings > 1) break;
      continue;
    }
    const sha = line.match(/^([0-9a-f]{64})\s+(\S.*?)\s*$/);
    if (sha) {
      rel.sha256.push({ hash: sha[1], file: sha[2] });
      continue;
    }
    const h = line.match(/^### (.+?)\s*$/);
    if (h) {
      group = GROUPS[h[1]] ?? null;
      continue;
    }
    const b = line.match(/^- (.+?)\s*$/);
    if (b && group) rel[group].push(b[1]);
  }
  return rel;
}

/** The SHA-256 of a released file by its exact name, or null. */
export function shaOf(rel: Release, file: string): string | null {
  return rel.sha256.find((s) => s.file === file)?.hash ?? null;
}

function semverDesc(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) if (pa[i] !== pb[i]) return pb[i] - pa[i];
  return 0;
}

let cache: Release[] | null = null;

/** All released versions, newest first. Files that do not parse are skipped. */
export function versions(): Release[] {
  if (cache) return cache;
  const dir = changelogDir();
  if (!dir) return (cache = []);
  const out: Release[] = [];
  for (const f of readdirSync(dir)) {
    if (!/^\d+\.\d+\.\d+\.md$/.test(f)) continue;
    try {
      const rel = parseRelease(readFileSync(resolve(dir, f), "utf8"));
      if (rel) out.push(rel);
    } catch {
      // unreadable file: skip it
    }
  }
  out.sort((a, b) => semverDesc(a.version, b.version));
  return (cache = out);
}

export function latest(): Release | null {
  return versions()[0] ?? null;
}

/** The anchor id of a version on the changes page: 1.6.2 -> v1-6-2. */
export function versionId(version: string): string {
  return "v" + version.replaceAll(".", "-");
}
