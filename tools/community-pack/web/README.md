# Community hub web page

A static page that browses the community catalog (`index.json`, the same file the game's
«Карты и моды» window reads) in a browser: tabs for maps, campaigns and mods, search, a list
ordered by rating, and a details pane per entry with its author, languages, required mods,
description, preview and download buttons. It mirrors `src/ui/hub_view.rs`; it does not install
anything, the player downloads the files and installs them in the game (the page says how).

Plain HTML, CSS and JavaScript. No build step, no framework, no external CDN, no fonts, no
analytics. Russian by default, with an English switch.

## Files

- `index.html`: the page.
- `hub.css`: the styles (dark stone and parchment, cream text, gold accents; two columns on a
  wide screen, stacked below 700px).
- `hub.js`: the catalog reader and the rendering. All catalog text goes into the page with
  `textContent` or property assignment, never `innerHTML`.
- `README.md`: this file.

Every file name is lowercase ASCII with one extension, so GitVerse Pages and GitHub Pages both take them.

## Deployment

The page reads `index.json` from its own folder. Put the four files and the catalog in one folder
and serve that folder:

- GitHub Pages or GitVerse Pages: copy the files beside `index.json` into the published folder.
- The files and previews named by the catalog are relative paths. They must sit next to
  `index.json`, or the page must be told where they are (next section).

The catalog's default source is the latest release of `indicozy/razdor-community`. Its `index.json`
and files are release assets, not part of this repository.

## Configuration

The catalog is always read from `./index.json`, next to the page.

Files and previews: a catalog path is either

- `https://…`: used as it is, or
- relative: resolved against the **files base**.

The files base is, by default, the page's own folder, so relative paths resolve next to
`index.json`. To keep the catalog and the page on one host but fetch the files from the
community release, set the base in `index.html`:

```html
<meta name="razdor-files-base" content="https://github.com/indicozy/razdor-community/releases/latest/download/">
```

The meta is not set by default. A relative value is resolved against the page's address.

## Error state

The page fetches `index.json` and parses it. If the fetch fails, the body is not JSON (a host's
«not found» page often comes back as HTML with status 200), or the format is not `format: 1`, the
page shows a Russian (or English) message with a link to the catalog on GitHub and a retry button.

## Checking it locally

```sh
python3 -m http.server 8000 --bind 127.0.0.1
```

Run it in a folder that holds the page, `index.json` and the catalog's files. Open
`http://127.0.0.1:8000/`. Deep links work: `#<entry id>` opens that entry, and back and forward
move between the entries picked.
