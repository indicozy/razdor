# App data compression (backlog 1609, part 2)

The owner (2026-11-14): Razdor's own app data should mostly be stored compressed (the game Razdor downloaded, the
hub's mods); a plain file always wins; the hub serves files already compressed so their SHA-256 match after download;
old folders are converted once, safely; tiny files and kinds that do not shrink are left alone.

## What is compressed

Measured on the Community Update install (132.8 MB, zlib level 1 as a stand-in for LZ4, which is a bit weaker):

| kind | files | bytes | compressed | ratio |
|---|---|---|---|---|
| .ugs | 48 | 63.9 MB | 10.7 MB | 6.0× |
| .lit | 340 | 30.0 MB | 5.2 MB | 5.7× |
| .raw (sounds) | 18 | 28.6 MB | 27.5 MB | 1.04× |
| .dll / .exe | 6 | 5.0 MB | 1.9 MB | ~2.7× |
| .bmp | 36 | 1.9 MB | 0.8 MB | 2.4× |
| .dtm (bzip2 already) | 24 | 1.2 MB | 1.2 MB | 1.0× |
| everything else | ~90 | ~1.8 MB | | |

Decision: only `.ugs` and `.lit` are stored compressed (94 MB → about 16–19 MB with LZ4; the install about
133 → 55–60 MB). Everything else stays plain: sounds, maps (`.DTm` is bzip2), ini, exe/dll (the integrity check hashes
the exe, and the original runs from it), bmp/tga/spi (small). Keeping ini and DTm plain leaves `is_install`, `read_ini`,
`english::discover`, `list_maps`, the save/replay map hashes and map import untouched, and an older Razdor still
recognises the folder.

Saves (bzip2 JSON), replays (appended in place), hub maps (`.DTm`) and the picture cache (LZ4 since 1609 part 1) are
out of scope.

## Format

A compressed file is `<name>.lz4` next to where `<name>` would be (e.g. `Graphics/Units/Knight.ugs.lz4`), in the
standard LZ4 frame format with the content size in the frame header (so the logical length is known without decoding).
Rust: `lz4_flex::frame`. Python (build.py): `lz4.frame.compress(…, store_size=True)`.

## Rules

1. **One resolver** (`dt::stored`, new): every read of a game file goes through it. Logical name → stored file: the
   plain file if it exists (case-insensitive, as `install::find`), else `<name>.lz4`. API: find, exists/is_file,
   logical length, read (decompressed bytes), list a directory by logical names (`.lz4` stripped, a name listed once).
   Decoders dispatch on the logical extension (`gfx::decode_uncached`).
2. **Plain wins.** When both forms exist the plain file is the truth: the converter recompresses from it, replaces the
   `.lz4`, then deletes the plain file. A patch import or a repair that writes plain files is therefore always correct.
3. **Ownership.** Razdor converts and writes `.lz4` only in a folder it owns:
   - Android/iOS: the app's `unpack_dir` (the only install root there).
   - Desktop: only a folder with Razdor's marker file (`.razdor-owned`, written when Razdor downloads or unpacks the
     game into `unpack_dir`). A `DiscordTimes` folder next to the exe without the marker may be the player's own copy
     that the original runs from: never touched. `RAZDOR_DT_DIR` and any other found install: never touched.
4. **Integrity** (`dt::integrity`) checks logical files: size and SHA-256 of the decompressed bytes (the manifest
   stays the original's). Repair writes plain files (rule 2 then recompresses them in an owned folder).
5. **Conversion** runs once per owned folder before the install loads, crash-safe: write `<name>.lz4.part`, fsync,
   verify by decoding and comparing the length/hash, rename to `<name>.lz4`, then delete the plain file. Leftover
   `.part` files are deleted at start. A progress notice on phones is a UI change (owner's screenshots).
6. **Mods** (`hub/mods.rs`): the overlay matches layers by logical name and removes the other stored form of a name it
   replaces (else a base's plain file beats a mod's `.lz4`). Mod manifests keep recording the stored bytes (what the
   hub sent). `layer_bytes`/`top_file` go through the resolver.
7. **Hub files are stored verbatim**, never recompressed (Python's and Rust's LZ4 encoders give different bytes):
   build.py ships `.ugs`/`.lit` as `.lz4` assets, the catalog's size/sha256 are the `.lz4`'s. Released clients that
   cannot read `.lz4` must not get such a catalog: a new index name (checked against what 1.5.1 reads), the owner's
   yes before publishing, and the GitVerse quota checked.
8. **Picture cache**: its key is path + length + mtime; key compressed files on the logical path and the decoded
   length so a conversion does not force a rebuild (or accept one rebuild, said in the changelog).

## Steps

1. Resolver + every reader through it (no writer changes; a plain install behaves exactly as before). Gate: a scratch
   copy of the install converted by a test helper reads byte-identical through the resolver for every manifest file;
   the integrity check passes on it; `community_maps_smoke` and `hub_real_catalog` pass with that copy as the install.
   Never run them on the symlinked installs in the community folder.
2. Ownership marker, the converter (rule 5) at start, unpack/import/download writing `.lz4` in owned folders,
   integrity on logical files. Separate fix, its own backlog entry: the repair has no ownership check today
   (`ui/game_import.rs` `start_repair`) and can write into a player's own install.
3. Mods overlay by logical names; build.py `.lz4` assets; new index name; publish after the owner's yes.

Open: how a hub mod's bytes sit on Android (`mods/<id>/game/` is hard links, or full copies where links fail; the kept
`archives/*.zip`): measure on a phone; if `game/` is copied there, that copy is the real cost, not the diff files.
