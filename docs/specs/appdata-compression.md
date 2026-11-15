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
   **Readers while it runs** (the race of the carry-over check, 2026-11-15): a single file is always readable
   (a found plain picture converted meanwhile reads its `.lz4`; `Stored::present` for a check of a found file).
   A reader that walks the base as a whole waits first: `stored::wait_converted(base)` at the start of
   `hub::mods::prepare_mod` (the mod build, its fingerprint and hard links), and a build that fails is tried once
   more after the wait (`active_install_dir_in`, `use_mod`), so a mod is not dropped for a file that moved. The
   wait is the conversion's time (measured in the carry-over run on the desktop: the conversion of 388 pictures
   ended at 2.1 s, the install loaded at 2.1 s; phones not measured; once per folder, and only with a mod active);
   the conversion holds its folder from the calling thread before it starts (`Converting`), so the wait sees it.
   Nothing else waits: `find`, `list` and `Stored::read` never do.
6. **Mods** (`hub/mods.rs`, the design of step 3): the overlay matches layers by logical name and removes the other stored
   form of a name it replaces, both ways (a base's plain link beats nothing once a mod's `.lz4` is laid; a base's
   `.lz4` goes for a mod's plain file). A mod's manifest and stamp name each file by its logical path, and its digest
   is of the decoded bytes, so either form matches. `layer_bytes`/`top_file` go through the resolver.
7. **Hub files are stored verbatim**, never recompressed (Python's and Rust's LZ4 encoders give different bytes) when the
   hub ships them compressed. Step 3 does not do that: the catalog, its SHAs and the index name are unchanged; Razdor
   compresses a mod's pictures itself after unpacking (see step 3). A catalog of `.lz4` assets (build.py) is still a
   later decision: released clients that cannot read `.lz4` must not get such a catalog (a new index name, the owner's
   yes before publishing, and the GitVerse quota checked).
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
3. **Mods, client side (done, backlog 1609)**. No catalog change: the catalog and its SHAs are the hub's as they are, and
   nothing is published.
   - A mod's zip (deflate) or plain file is unpacked into `files.tmp/` (`Hub::write_mod`); then every `.ugs`/`.lit` in it is
     compressed (`mods::compress_pictures` → `stored::compress_folder`, the converter's rules: `<name>.lz4.part`,
     verified, renamed, the plain file removed; a failure is logged and the plain file stays). The manifest is written
     from the files: `file_entries` lists the logical path (`Pictures/A.lit`), the size of the decoded bytes
     (`logical_len`) and their SHA-256. The downloaded zip is removed after it is unpacked; no zip is kept
     (`keep_archives` is gone; `repair_mod` downloads and checks again).
   - Checks read a file in either form: `damaged_files`/`matches_entry` find the entry with `stored::find` and compare the
     decoded length and SHA-256 (a plain file is still streamed, as before).
   - **Installed mods** (`prepare_mod`, at each play, for every layer of the chain): after the asked mod's damage check
     passes, the plain pictures of each layer are compressed in place, and that layer's `archives/` (the zips kept by
     older installs) is removed. A plain file is kept until its `.lz4` is verified; the manifest still matches (logical
     names and digests), so nothing is re-downloaded.
   - **Stamp**: the stamp keeps the names as they are laid (the stored ones, `Pictures/A.lit.lz4`) with the logical
     digests. So an installed mod whose pictures were converted gets its merged folder built once more (its `game/` then
     holds the `.lz4` copies, not the plain ones); after that the stamp is stable and the folder is reused at each play.
     A mod with no pictures is not rebuilt by this (its stamp names do not change).
   - **Layering** (`build`): a layer's file is laid under its stored name, and every other stored form of the same logical
     name in the merged folder goes first (`drop_other_stored_form`, both ways, ignoring case). A `.lz4` of a name the base
     has as a plain hard link therefore replaces the link; with no hard links (the read-through layout) the lookup finds the
     mod's `.lz4` before the base's file.
   - Mod records in `installed.json` keep the logical names (what the mod ships), as before.
   - The merged folder never copies the base: hard links where the file system allows them, else the base is read
     through (`install::set_overlay`, b9b1f04e). On the Android emulator a played mod's `game/` held only the mod's own
     files.
   - Not in step 3: a catalog that ships `.lz4` assets (build.py), which would save the download too; see rule 7.

Decided (the owner, 2026-11-14): desktop installs downloaded before the marker existed stay as they are (plain, no
repair button until downloaded again); phones are the priority for compression.
