// The latest release's asset sizes, read from the GitHub API at build time (3 tries, 2 s apart).
// A failed fetch gives no sizes: the pages then leave the size out, they never invent one.
import type { Lang } from "../i18n";

const API = "https://api.github.com/repos/indicozy/razdor/releases/latest";

let pending: Promise<Map<string, number> | null> | null = null;

/** The file name -> size in bytes of the latest release's assets, or null when the API cannot be read. */
export function assetSizes(): Promise<Map<string, number> | null> {
  if (!pending) {
    pending = (async () => {
      for (let attempt = 0; attempt < 3; attempt++) {
        if (attempt > 0) await new Promise((resolve) => setTimeout(resolve, 2000));
        try {
          const res = await fetch(API, {
            headers: { accept: "application/vnd.github+json", "user-agent": "razdor-site-build" },
            signal: AbortSignal.timeout(15000),
          });
          if (!res.ok) continue;
          const data = (await res.json()) as { assets?: { name: string; size: number }[] };
          if (!Array.isArray(data.assets)) continue;
          return new Map(data.assets.map((a) => [a.name, a.size] as [string, number]));
        } catch {
          // network or parse failure: try again, then give up
        }
      }
      return null;
    })();
  }
  return pending;
}

/** 12.3 MB -> "12,3 МБ" (ru) or "12.3 MB" (en). */
export function formatSize(bytes: number, lang: Lang): string {
  const mb = (bytes / 1048576).toFixed(1);
  return lang === "ru" ? `${mb.replace(".", ",")} МБ` : `${mb} MB`;
}

// The downloads counter: the sum of download_count over the assets of every release (GitHub's list, 100 per page).
// Any page that cannot be read gives null, and the pages then leave the counter out instead of showing a partial sum.
const RELEASES_API = "https://api.github.com/repos/indicozy/razdor/releases";

let totalPending: Promise<number | null> | null = null;

async function releasesPage(page: number): Promise<{ assets?: { download_count?: number }[] }[] | null> {
  for (let attempt = 0; attempt < 3; attempt++) {
    if (attempt > 0) await new Promise((resolve) => setTimeout(resolve, 2000));
    try {
      const res = await fetch(`${RELEASES_API}?per_page=100&page=${page}`, {
        headers: { accept: "application/vnd.github+json", "user-agent": "razdor-site-build" },
        signal: AbortSignal.timeout(15000),
      });
      if (!res.ok) continue;
      const data = (await res.json()) as unknown;
      if (Array.isArray(data)) return data;
    } catch {
      // network or parse failure: try again, then give up
    }
  }
  return null;
}

/** The total downloads of all releases, or null when the API cannot be read. */
export function downloadTotal(): Promise<number | null> {
  if (!totalPending) {
    totalPending = (async () => {
      let total = 0;
      for (let page = 1; page <= 20; page++) {
        const rels = await releasesPage(page);
        if (!rels) return null;
        if (rels.length === 0) return total;
        for (const rel of rels) for (const a of rel.assets ?? []) total += a.download_count ?? 0;
      }
      return total;
    })();
  }
  return totalPending;
}
