// The counter's play stats: three small tables (popular maps, hardest maps, games) from the API.
// The block stays hidden when the fetch fails, is slow (6 s) or the page has no JS. No player counts.
const API = "https://razdor.indicozy.dev/api/v1/public/stats";

interface Stats {
  top_maps: { map: string; starts: number }[];
  hard_maps: { map: string; win_rate: number }[];
  top_games: { game: string }[];
}
type Titles = Record<string, { ru: string; en: string }>;

const fill = (block: HTMLElement, board: string, rows: [string, string][], head: string[]) => {
  const table = block.querySelector<HTMLTableElement>(`[data-board="${board}"]`);
  if (!table) return false;
  const tbody = table.tBodies[0];
  tbody.replaceChildren(
    ...rows.map(([a, b]) => {
      const tr = document.createElement("tr");
      const ta = document.createElement("td");
      ta.textContent = a;
      tr.append(ta);
      if (head.length > 1) {
        const tb = document.createElement("td");
        tb.className = "n";
        tb.textContent = b;
        tr.append(tb);
      }
      return tr;
    }),
  );
  table.hidden = rows.length === 0;
  return rows.length > 0;
};

async function run(): Promise<void> {
  const block = document.getElementById("play-stats");
  if (!block) return;
  let stats: Stats;
  try {
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(), 6000);
    const res = await fetch(API, { signal: ctl.signal });
    clearTimeout(timer);
    if (!res.ok) return;
    stats = (await res.json()) as Stats;
  } catch {
    return;
  }
  const lang = block.dataset.lang === "en" ? "en" : "ru";
  let titles: Titles = {};
  try {
    titles = JSON.parse(document.getElementById("map-names")?.textContent || "{}") as Titles;
  } catch {
    titles = {};
  }
  const title = (id: string) => {
    const t = titles[id];
    const s = t ? (lang === "en" ? t.en || t.ru : t.ru || t.en) : "";
    return s || id.replace(/_/g, " ");
  };
  const wins = block.dataset.wins ?? "";
  const original = block.dataset.original ?? "";
  const maps = fill(
    block,
    "top-maps",
    (stats.top_maps ?? []).map((m) => [title(m.map), String(m.starts)]),
    ["map", "n"],
  );
  const hard = fill(
    block,
    "hard-maps",
    (stats.hard_maps ?? []).map((m) => [title(m.map), `${wins} ${Math.round(m.win_rate * 100)}%`]),
    ["map", "n"],
  );
  const games = fill(
    block,
    "games",
    (stats.top_games ?? []).map((g) => [g.game === "dt" ? original : title(g.game), ""]),
    ["game"],
  );
  block.hidden = !(maps || hard || games);
}

run();
