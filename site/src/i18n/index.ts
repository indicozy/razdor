import { ru } from "./ru";
import { en } from "./en";
import { ruPlural } from "../lib/i18n";
import type { CatalogCounts } from "../lib/catalog";

export type Lang = "ru" | "en";

export function strings(lang: Lang) {
  return lang === "ru" ? ru : en;
}

/**
 * A site URL in one language: href("en", "download/") -> "<base>en/download/".
 * The wiki has its own English part (/wiki/en/); external links stay as they are.
 */
export function href(lang: Lang, rel: string): string {
  const B = import.meta.env.BASE_URL;
  if (/^https?:/.test(rel)) return rel;
  if (rel.startsWith("wiki/")) return `${B}wiki/${lang === "en" ? "en/" : ""}${rel.slice(5)}`;
  if (rel === "wiki") return `${B}wiki/${lang === "en" ? "en/" : ""}`;
  return `${B}${lang === "en" ? "en/" : ""}${rel.replace(/^\//, "")}`;
}

/** The Hub counts line for the landing and the community page; null when the catalog is missing. */
export function hubLine(lang: Lang, c: CatalogCounts | null): string | null {
  if (!c) return null;
  if (lang === "en") return `The Hub has ${c.maps} maps and ${c.mods} mods from ${c.authors} authors.`;
  return `В хабе ${c.maps} ${ruPlural(c.maps, ["карта", "карты", "карт"])} и ${c.mods} ${ruPlural(c.mods, ["мод", "мода", "модов"])} от ${c.authors} ${ruPlural(c.authors, ["автор", "автора", "авторов"])}.`;
}
