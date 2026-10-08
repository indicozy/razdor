# Auto updates — design

2026-10-08. Razdor finds a newer release on GitHub, asks (or not, by the setting), downloads
it, checks it and puts it in its own place. The release pipeline stays as it is.

## What the player sees

- **Settings → Advanced… → Updates:** *Ask* (default), *Always* or *Off*, and a **Check now**
  button with its result next to it ("Checking…", "Up to date", "Could not check",
  "Razdor X.Y.Z is out").
- **Ask:** when a newer release is found, a window "Razdor X.Y.Z is out (you have A.B.C)"
  and the changes since this version, with **Update**, **Always update** (the same, and the
  setting becomes Always) and **Later** (until
  the next start). It opens only at a calm moment: the title screen, or the world map with
  no dialog, key list, console or battle pending. **Update** downloads in the background
  while the game goes on; once it is in place a window says it starts the next time
  Razdor starts, with **Restart now** on the title screen (in a game only **OK**, so no
  progress is lost). A failed download says why, with a button to the release page.
- **Always:** the same download with no window. The title screen shows a line "Razdor
  X.Y.Z is installed and starts next time".
- **Off:** no request at all; **Check now** still works.
- Nothing is ever shown for a check that failed or found nothing, except after **Check now**.

## How

`razdor::update` (library, no UI) and `ui::update_view` (the windows).

- **Request:** after the window is open, a background thread runs the system `curl`
  (`-fsSL --max-time 20`) against
  `https://api.github.com/repos/indicozy/razdor/releases/latest`; the game never waits on it.
  On Windows curl starts with `CREATE_NO_WINDOW`. The reply's `tag_name`, `html_url` and
  `assets` (name, `browser_download_url`) are read with serde_json. Newer means a greater
  `X.Y.Z` than `CARGO_PKG_VERSION`.
- **Asset:** `Razdor.exe` (Windows), `razdor-macos` (macOS), `razdor` (Linux), and
  `SHA256SUMS`.
- **Download:** curl into `<program folder>/.<name>.update` and the sums; the SHA-256
  (the `sha2` crate, pure Rust) must match the file's line in `SHA256SUMS`, or the file is
  deleted and the install fails.
- **Swap:** Unix: mode 755, then rename over the program (the running copy keeps its inode).
  Windows: rename the program to `<name>.old`, rename the new one into place (back again if
  that fails); `.old` is deleted at the next start. The program path is taken at start,
  before any swap.
- **Restart now:** the instance lock is released, the program starts again with the same
  arguments, and this one exits.
- **Not checked:** a program under a `target` folder (a `cargo` build), unless
  `RAZDOR_UPDATE_AS=<X.Y.Z>` is set: it pretends to be that version (to test the flow against
  real releases). A folder that cannot be written fails with the OS's reason.
- **Setting:** `updates: "ask" | "always" | "off"` in the settings file, default ask.

## Testing

Unit tests: version parsing and order, the asset per platform, the API reply, a
`SHA256SUMS` line, the hash check, the swap in a temporary folder. A snapshot scene
`update` shows the window with a made-up release. By hand: a copy of the program with
`RAZDOR_UPDATE_AS=0.3.0` updates to the latest release.

## Not in it

Delta updates, "skip this version", signatures beyond the release's SHA256SUMS, rollback.
