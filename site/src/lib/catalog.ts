// The community catalog (the Hub's index.json) read at build time for the community page and the landing's counts.
// A failed fetch is not an error: the callers show their fallback text and links.
export interface CatalogText {
  ru: string;
  en: string;
}

export interface CatalogFile {
  path: string;
  name?: string;
  size?: number;
  sha256?: string;
}

export interface CatalogEntry {
  id: string;
  kind: "map" | "campaign" | "mod";
  title: CatalogText;
  author: string;
  version: string;
  description: CatalogText;
  added: string;
  source_url: string;
  picture?: CatalogFile;
  preview?: CatalogFile;
}

export interface Catalog {
  format: number;
  entries: CatalogEntry[];
}

export interface CatalogCounts {
  maps: number;
  mods: number;
  authors: number;
}

const INDEX_URL = "https://github.com/indicozy/razdor-community/releases/latest/download/index.json";
const ASSETS = "https://github.com/indicozy/razdor-community/releases/latest/download/";

let pending: Promise<Catalog | null> | null = null;

/** The catalog, or null when it cannot be fetched or does not parse. Fetched once per build. */
export function catalog(): Promise<Catalog | null> {
  if (!pending) {
    pending = (async () => {
      try {
        const res = await fetch(INDEX_URL, { signal: AbortSignal.timeout(20000) });
        if (!res.ok) return null;
        const data = (await res.json()) as Partial<Catalog>;
        if (!data || !Array.isArray(data.entries)) return null;
        return { format: data.format ?? 0, entries: data.entries };
      } catch {
        return null;
      }
    })();
  }
  return pending;
}

export function counts(c: Catalog): CatalogCounts {
  return {
    maps: c.entries.filter((e) => e.kind === "map" || e.kind === "campaign").length,
    mods: c.entries.filter((e) => e.kind === "mod").length,
    authors: new Set(c.entries.map((e) => e.author)).size,
  };
}

/** The picture of an entry as a URL on the catalog's release, or null. */
export function pictureUrl(e: CatalogEntry): string | null {
  const f = e.picture ?? e.preview;
  return f?.path ? ASSETS + f.path : null;
}
