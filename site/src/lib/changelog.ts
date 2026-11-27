// Reads the release notes (changelog/X.Y.Z.md) at build time for the changes pages and «Что нового».
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

export interface Release {
  version: string;
  date: string;
  added: string[];
  changed: string[];
  fixed: string[];
}

const GROUPS: Record<string, keyof Omit<Release, "version" | "date">> = {
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
  const rel: Release = { version: head[1], date: head[2], added: [], changed: [], fixed: [] };
  let group: keyof Omit<Release, "version" | "date"> | null = null;
  for (const line of text.split("\n")) {
    if (line.startsWith("## ")) break; // only the first version heading's block
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
