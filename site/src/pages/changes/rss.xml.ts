import { versions, versionId } from "../../lib/changelog";
import { ru } from "../../i18n/ru";

const SITE = "https://razdor.indicozy.dev";
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

// RSS 2.0 of the last 20 versions (release notes are in Russian).
export function GET() {
  const items = versions()
    .slice(0, 20)
    .map((r) => {
      const link = `${SITE}/changes/#${versionId(r.version)}`;
      const lines = [...r.added.map((l) => `${ru.changes.groups.added}: ${l}`), ...r.changed.map((l) => `${ru.changes.groups.changed}: ${l}`), ...r.fixed.map((l) => `${ru.changes.groups.fixed}: ${l}`)];
      return `    <item>
      <title>Razdor ${esc(r.version)}</title>
      <link>${link}</link>
      <guid isPermaLink="true">${link}</guid>
      <pubDate>${new Date(r.date + "T12:00:00Z").toUTCString()}</pubDate>
      <description>${esc(lines.join("\n"))}</description>
    </item>`;
    })
    .join("\n");
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Razdor: изменения</title>
    <link>${SITE}/changes/</link>
    <description>Выпуски Razdor, новые сверху.</description>
    <language>ru</language>
${items}
  </channel>
</rss>
`;
  return new Response(body, { headers: { "Content-Type": "application/rss+xml; charset=utf-8" } });
}
