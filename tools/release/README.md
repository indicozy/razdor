# Release tools

`update_check.py`: checks that the previous release updates to the new one (release step 6b), and
that the data an older release left carries over to the new program (the carry-over check, steps 3b
and 6b).

After the release (the published binary is the new one):

    $(ai-resources prefix) python3 -I tools/release/update_check.py --from v1.5.1 --to v1.5.2

Before the tag (the locally built program; the carry-over check only, no self-update, no Android):

    $(ai-resources prefix) python3 -I tools/release/update_check.py --from v1.5.1 --from v1.5.0 --from v1.4.1 \
        --new-bin target/release/razdor

- `--to` must be the newest published release (drafts and pre-releases aside): the updater only
  offers the newest. `--from` must be an older published release; give it once per old release.
  Each `--from` runs in its own folder under the scratch folder.
- Desktop (Linux) update: the old program, `updates = always`, puts the new one in place; the file must
  match the new release's `SHA256SUMS` and the updated program must start and log the new version.
  Android: both APKs verify, the same certificate, the same package, a versionCode that does not go down.
- Carry-over (Linux, every `--from`), one folder set per old release:
  1. the old release's own Linux program, updates off, on a fresh HOME and XDG folders inside the
     scratch folder: its game folder (a copy of `RAZDOR_DT_DIR` next to the program, unmarked, the way
     the old release leaves it), a Hub mod laid out as the release writes it (`hub/mods/<id>/files`, the
     kept `archives/1.zip`, `manifest.json`, `installed.json`, `active.json`; six pictures of the Evolution
     install, `RAZDOR_COMMUNITY_DIR`), a map started from the scenario list (its session slot is the save),
     and the picture cache filled by 75 s of play; then the mod made active and the same session
     continued with it for 40 s;
  2. the new program on the same folders: the logical SHA-256 of each game and mod file must not change
     (a `.lz4` counts as its decoded bytes), the session is answered with Enter at «Continue the game?»,
     the picture cache must be in the new format, the logs gain no error line that the old run's log lacks,
     the game folder (no marker) must be left alone, and a second start changes nothing;
  3. the harness marks the game folder as Razdor's (the `.razdor-owned` a copy Razdor unpacked has, labelled
     as the test's own doing) and the first-start conversion runs: its time, its screenshots, the same digest
     check, and a second start that changes nothing.
  The screenshots are kept in `<scratch>/shots/<from>/` (`A-*.png` the first start, `*-owned/` the conversion).
- Needs: network (the GitHub API and downloads, anonymous: the repo is public), `Xvfb` and ImageMagick's
  `import` (the screenshots), `libX11` and `libXtst` (the key presses on the private display), `Xvfb`
  (Linux), the Android SDK build-tools with `apksigner` and `aapt2` (`ANDROID_HOME` or `ANDROID_SDK_ROOT`;
  else `/mnt/ai_models/razdor-scratch/android-apk/sdk`), and Java for `apksigner`. `RAZDOR_DT_DIR` comes
  from the environment or the repo's `.env`; it is never printed. `RAZDOR_COMMUNITY_DIR` (or `--mod-src`)
  names the mod source; without it the carry-over check runs without the test mod and says so.
- The carry-over check uses ALSA's null device (the sandbox has no sound card; the game's audio thread
  panics without one), `updates = off`, and no network except the game's own catalog fetches.
- Scratch folder: `--scratch`, else `$RAZDOR_SCRATCH`, else `/mnt/ai_models/razdor-scratch/update-check/<to>`
  (`pre-tag` before the tag), else a new private temp folder. It holds downloaded programs, so it must be a
  real folder owned by this user and not writable by group or others (the tool refuses a symlink or a
  looser folder, and sets it to 0700). On success the per-release folders are removed unless `--keep`;
  the screenshots stay, so the root folder stays too. The processes the tool starts are always stopped.
- Use `$(ai-resources prefix)` (CPUs 8-19 at nice 19 on the desk): the game renders in software under Xvfb.
  A carry-over run takes about 6 minutes per `--from`.
- Exit: 0 all checks pass (INFO rows are not failures), 1 a check failed (the previous release does not
  update to this one, or its data does not carry over), 2 the check could not run (network, a tool, an
  input error, or the old release's own data could not be made).
- Not covered: Windows, macOS, iOS, phones (the first-start conversion there is always on: an emulator
  run is separate), and a player's real install (its saves, its Hub, the phone's install dialog). The
  conversion's screens are the harness's own screenshots on a private display, not a player's.
