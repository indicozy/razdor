import type { Lang } from "../i18n";

/** 2026-11-26 -> "26 ноября 2026" (ru) or "November 26, 2026" (en). */
export function longDate(lang: Lang, iso: string): string {
  return new Intl.DateTimeFormat(lang === "ru" ? "ru-RU" : "en-US", {
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  })
    .format(new Date(iso + "T12:00:00Z"))
    .replace(" г.", "");
}

/** The Hub's release-download address of a file of the main repo (the GitHub one and its own copy). */
export const RELEASES = "https://github.com/indicozy/razdor/releases/latest/download/";
export const MIRROR = "https://razdor.indicozy.dev/hub/indicozy/razdor/releases/latest/download/";
