# Razdor player wiki

Starlight (Astro) site, deployed by `.github/workflows/pages.yml` together with the hand-made site
from the `gh-pages` branch, at its `wiki/`: https://indicozy.github.io/razdor/wiki/

- Russian pages at the root of `src/content/docs/`, English under `en/`. Each section
  folder has an `index.md`; its sidebar order is set in the page frontmatter.
- Screenshots go in `src/assets/shots/` as `.webp`. A page refers to one with a relative
  path such as `../../../assets/shots/<name>.webp` (one more `../` under `en/`), so Astro optimises it at build time.
- Pinned versions: `npm install --save-exact`. Build: `npm ci && npm run build`
  (Pagefind search is built with it).
- Fonts and icons are copied from the main site's `gh-pages` branch (`fonts/`, `icon-192.png`,
  `favicon.ico`); the licence texts sit next to the fonts.

Helpers in `tools/wiki/`: `shot.sh` (an off-screen screenshot of a scene),
`qwen-shots.sh` (a batch of screenshots checked by the local Qwen) and `gemma.py` (drafts and
English translations with the local model through Ollama).
