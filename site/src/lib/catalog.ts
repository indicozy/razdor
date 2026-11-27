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

// GitHub first, then the owner's mirror (the same order as the app: docs/specs/own-mirror-2026-11-24.md). GitHub's
// release downloads sometimes answer 403 to build machines.
const INDEX_URLS = [
  "https://github.com/indicozy/razdor-community/releases/latest/download/index.json",
  "https://razdor.indicozy.dev/hub/indicozy/razdor-community/releases/latest/download/index.json",
];
// The catalog's pictures come from the owner's mirror: it serves the same release files, it is the site's own host,
// and GitHub's release downloads sometimes refuse plain image requests (403).
const ASSETS = "https://razdor.indicozy.dev/hub/indicozy/razdor-community/releases/latest/download/";

let pending: Promise<Catalog | null> | null = null;

/** The catalog, or null when it cannot be fetched or does not parse (4 tries, GitHub and the mirror in turn). Fetched once per build. */
export function catalog(): Promise<Catalog | null> {
  if (!pending) {
    pending = (async () => {
      // 4 tries, 2 s apart, GitHub and the mirror in turn; then the fallback text
      for (let attempt = 0; attempt < 4; attempt++) {
        if (attempt > 0) await new Promise((resolve) => setTimeout(resolve, 2000));
        try {
          const url = INDEX_URLS[attempt % INDEX_URLS.length];
          const res = await fetch(url, { signal: AbortSignal.timeout(20000) });
          if (!res.ok) continue;
          const data = (await res.json()) as Partial<Catalog>;
          if (!data || !Array.isArray(data.entries)) continue;
          return { format: data.format ?? 0, entries: data.entries };
        } catch {
          // network or parse failure: try again
        }
      }
      return null;
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
