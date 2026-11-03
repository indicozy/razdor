# Package versions: every version kept, a version picker, an "update" mark

Status: design, 2026-11-03. Branch `v2-packages`.

## Why

An author (the owner first, with Пепельная Марка 0.1.0 → 0.2.0) must be able to update a
package without breaking anyone's game, and the player should notice that something new
arrived and still be free to keep playing the version they like. Today the Hub keeps one
version per package (`installed.json` is keyed by id with one `version`), a catalog update
deletes the older `.rzpack` (`src/hub.rs` around line 814), and the new-game list shows only
the title, so an update is invisible and an old save can lose its file.

## Goals (the owner's decisions)

1. **Every version is kept.** Installing or importing a newer version never deletes an older
   one: it is added next to it (`<hub>/packages/<id>-<version>.rzpack`, already the naming).
   Every installed version stays playable. Old saves keep loading because their exact file
   stays.
2. **A version dropdown, in both places:** on the package's card in the Hub and on the
   package map's page in the new-game screen. It is shown only when more than one version is
   installed (its presence is what tells the player there are several versions).
3. **The dropdown's default:** the version this player last played of this package; if they
   never played it, the newest version.
4. **An "update" mark** means "a newer version is here that this player has not played yet".
   It shows on the map list (new-game screen) and on the single map's page (and on the Hub
   card, see the builder's calls). It goes away once the player starts the newest version.
   Having several versions is not marked; the dropdown shows that.

5. **The new-game list shows every installed package** (the owner's yes, 2026-11-03), always,
   not only the one opened from the Hub this session: one entry per package (a campaign shows
   its campaign title), with the version dropdown on its page. Today a package enters the list
   only through `keep_pack_map` after Hub «Play», and the code assumes one package entry
   (`keep_pack_map`, `open_pack`'s `position(pack.is_some())`); that changes. Hub «Play» still
   works and lands on that package's entry with the card's chosen version.
6. **An «Install X.Y.Z» button on an installed package's Hub card** (the owner's yes,
   2026-11-03), shown only when the catalog lists a version the player does not have. It
   installs that version next to the old ones (never deletes them). The mark's trigger stays
   installed versions only: a catalog version not yet installed is announced by this button,
   not by the mark.

## Non-goals

- No game rule changes. This is Hub and UI work only; no experimental toggle.
- No changelog/"what's new" text inside the game.
- No per-version removal UI (see below).
- No changes to how saves find their package (by hash through `pack::trust`); it already
  works once the old file is kept.

## Anti-goals

- A player losing the ability to load a save because an update replaced its file.
- An update the player cannot see without opening the Hub.
- The default silently moving a player who is mid-way through an older version onto the new
  one (hence "last played" wins over "newest").

## Constraints (project rules)

- Every changed screen is a UI change: off-screen screenshots of each (desktop and the phones
  it touches) go to the owner, and nothing is pushed until the owner says yes. Run the UI
  regression check and the `mobile-ui` check (no text under 12 sp, no touch target under
  40 dp). Phone layout stays the same layout, modestly bigger; no full-screen pickers.
- UI texts in `data/lang/ru/*.txt` (Russian) with English keys as elsewhere in `src/ui`.
- Changelog line (Russian, plain) in `changelog/unreleased.md`; wiki page for packages updated
  (ru and en) via the `wiki` skill; the relevant `docs/flows/` file updated.
- `installed.json` written by older builds must still load (serde defaults); an old record
  with one `version`/`files` becomes a one-version list.

## Decided details

- "Played" means the player started a new game (or a campaign) on that version. Loading a
  save does not count. Starting an older version does not clear the mark; only starting the
  newest does.
- The choice in the dropdown and "last played" are one remembered value per package, shared by
  the Hub card and the new-game page: picking a version in one place shows it in the other.
- A save always opens on the version it was started on; the dropdown only affects new games.
- Removing a package in the Hub removes all its installed versions.
- The mark's baseline (Claude's call): the mark shows only when the newest installed version
  has not been played and an older version is installed. A first install never shows it. On
  the first run of this build, each package's single existing version counts as played, so
  existing players see no spurious marks.
- Same id and version, different bytes (Claude's call): refused, for imports and catalog
  installs alike, with a message telling the author to raise the version (an overwrite would
  break saves keyed by the old file's hash). The same bytes again is a no-op.
- Version order: by the manifest's version compared as semver (numeric parts), falling back to
  text comparison for anything that does not parse.

## Left to the builder

- The data shape: e.g. `InstalledEntry` gaining a list of versions (each with its file and
  `unchecked` flag and imported `entry`), plus a per-package `last_played` / `seen_newest`
  record, in `installed.json` or a small sibling file. Keep it backward compatible.
- Where the "last played" write happens (where a package game starts: `pack_play`).
- The look of the mark and the dropdown, within the existing widgets of the Hub and the
  new-game screen; shown to the owner as screenshots before the push.
- Whether the Hub card also carries the mark (recommended: yes, the same mark, so the update
  is seen wherever the package appears).
- Tests: installing/importing a second version keeps the first file and both records; the
  default pick (never played → newest, played → last played); the mark set on a new version
  and cleared by starting the newest only; an old `installed.json` loads.
