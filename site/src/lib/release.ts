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
