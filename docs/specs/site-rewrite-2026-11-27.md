# Website rewrite (2026-11-27)

The owner's decisions from the grilling of 2026-11-27. Branch `site-rewrite-2026-11-27`.
The current site is hand-written HTML on `gh-pages` (index, en, install, a hand-copied changelog).

## Round 2 changes (the owner, 2026-11-27, after the first low-fi mockups)

- The journey is not a big stage. Desktop: a narrow road strip on the left, about 25 % of the width, fixed,
  fading into the content (no hard edge); the whole landing's sections (first screen to the final download) are
  its stops, and the hero walks down it as the page scrolls. The battle is a section in the content column; the
  strip shows the two armies meeting. Phone: the road is a muted background behind the content, the hero in the
  gaps between sections, never under text.
- Look: a modern layout in 2000s PC-game dress with DT's colours and fonts: green marble, silver bevelled frames,
  gold Becker headings, brown text boxes, fire-orange raised buttons, DT-like title strips on cards, crisp
  (pixelated) sprites, a light CRT touch (scanlines, vignette) on pictures and the strip only, never on text.
  No DT window art as borders (colours, fonts and shapes only).
- Fire: DT's own animated fire effect (S-Fire) on torches, the burning village and the battle, at pixel size;
  not realistic video flames.

## Round 3–4 changes (the owner, 2026-11-27)

- Desktop: the content is a centred column (~1000 px); the road strip is in the RIGHT margin, fading leftwards
  into the content. Below ~1300 px viewport width the road becomes the muted background, as on phones.
- Direction C «Карта» picked (over «Летопись» and «Геральдика»): no frames or panels. Pictures dissolve into the
  page through fog-of-war masks (a DT mechanic), text sits on the dark map ground, headings in Becker with a
  hand-drawn gold underline, a dashed gold route line links the sections, map-legend captions in Kurale italic,
  Hub items as banner flags pinned on a fogged map. The game-UI square-corner rule does not apply to the site.

## Audience and look

- For DT veterans first (download right away), newcomers second (scroll to see the game).
- Dark, modern game landing. DT art and sprites as imagery; no main-menu layout (no ovals, no
  marble windows). The owner has Aterdux's permission to use DT's raw art on the site (note it on
  the licence page and in `site/README.md`).

## Tech

- Astro in `site/` on `main`'s line, built by `.github/workflows/pages.yml` together with `wiki/`.
  `gh-pages` is no longer hand-edited (retired once the new site ships).
- Generated: the changes pages from `changelog/*.md` (+ RSS/Atom), the latest version and download
  links from the release data, the Hub catalog from the `razdor-community` catalog (a catalog
  release triggers a rebuild).
- Static HTML that works with JS off. Small scripts only: the platform pick for the download
  button, the gallery, the YouTube click-to-load facade, the scroll journey fallback and the
  battle. No framework runtime. No live player count.
- The wiki (Starlight) gets the new palette, fonts and the shared header; its layout stays.

## Languages and addresses

- Russian at `/`, English at `/en/…`, page for page; the switch links to the same page.
  `hreflang` alternates on every page. Old `en.html`, `install.html`, `install-en.html` redirect.
- `razdor.indicozy.dev` is canonical; GitHub Pages is a full copy whose canonical points there.
- On razdor.indicozy.dev, nginx redirects only the bare `/`: `Accept-Language` led by ru/uk/be/kk
  stays, anything else gets a 302 to `/en/` (`Vary: Accept-Language, Cookie`). A `lang` cookie, set by
  the site's language link (a tiny inline script: it sets `lang=ru` or `lang=en` for a year, then the
  link is followed; with no JS the link still works), beats the header. Pages keeps a small
  "English version" hint instead. Installing the nginx change needs the owner's root approval.

## Landing page

1. First screen: «Времена раздора. На любом устройстве.» (EN "Discord Times. On any device."),
   one sentence under it, the download button for the detected platform (version, size; other
   platforms small under it; "Игру Razdor скачает сам"), «Смотреть трейлер». The hero stands at
   the castle. The download stays pinned in the header after the first screen.
2. Scroll journey: one tall section (~4 screens, 5 stops) with a sticky stage, native scroll, no
   scroll-jacking. The hero walks a road (DT sprites, walk cycle) through real moments of
   «Раменское королевство»: castle → road (the world map) → village/signpost (Hub, maps, mods) →
   battle → the ending (final download). The "why Razdor" points and the 3-step install strip are
   the stops' captions. Desktop: stage on the left two thirds, captions on the right. Phone
   portrait: stage full width in the top ~60 %, a fixed caption card under it; the road runs down.
   Only transform and opacity animate. Without scroll-driven animation support, JS off or
   `prefers-reduced-motion`: stacked stills with the same captions.
3. Battle stop: scripted "your turn" on a 2×6 field with DT sprites. Tap an enemy, the attack
   plays, damage floats, the enemy answers; 2–3 exchanges, always a victory. «Пропустить», and it
   auto-plays if the visitor keeps scrolling. ~5–10 KB JS. Fallback: before/after stills.
   (A real WASM battle of `src/rules` may come later as its own demo page.)
4. After the journey: Community (Hub counts baked at build, Discord, credits), What's new (latest
   version, link to Changes), Thorny Times capsule and credit, footer (GitHub, wiki, licence and
   credits, language switch). The then-and-now slider is optional.

## Other pages

- `/download/`: a card per platform (Windows, Windows 7/8 zip, Linux, macOS, Android APK, iOS IPA
  with AltStore/Sideloadly): file, size, version, SHA-256; GitHub first, the mirror under it.
  3-step install, "no game? Razdor downloads it", log and save locations, older versions → GitHub.
- `/features/`: grouped blocks with a picture each (the original game; any screen and device;
  Razdor's extras: Hub, cloud saves, «Свой бой», «Максимум», auto-updates, experimental switches;
  languages and fonts), each linking to its wiki page.
- `/community/`: Discord, the Hub catalog (picture, author, description per map or mod), adding
  your map, contributing (GitHub, CLA, translations).
- `/changes/`: every version, newest first, an anchor per version, RSS/Atom.
- `/faq/`: ~10 real questions from Discord reports and the wiki FAQ, short answers linking to the wiki.

## Process

- Low-fi mockup first (grey boxes, storyboard of the scroll journey on desktop and phone, the
  fallback, the other pages as wireframes) in `/mnt/ai_models/razdor-scratch/ui-mockups/site-2026-11-27/`;
  the owner approves before the build.
- Design checklists read once for this work: impeccable.style (/slop) and tasteskill.dev; the
  `frontend-design` skill for direction. Not installed as skills.
- All public text through the humanizer before it ships. Images checked with imgtune once real art is in.
