# Release tools

Checks run by the release skill on a Razdor release. Plain Python 3, standard library only; run them with
`python3 -I` from the repository root.

| Script | What it checks | Release step |
|---|---|---|
| `update_check.py` | the desktop update (the previous release puts the new one in place) and the data carry-over (saves, mods, the picture cache) | 3b, 6b |
| `from_list.py` | prints the `--from` list for `update_check.py` from the download stats | 3b, 6b |
| `android_update_check.py` | the Android APK over the previous one: signatures, package, versionCode, and a headless emulator install with the data kept | 6c (after the release) |
| `rollback.py` | withdraws a bad release from the update feeds (dry run by default) | Rollback |
| `hub_candidate_check.py` | a candidate hub catalog (the publish dry run's folder), served on 127.0.0.1 over TLS, read and used by the old Linux releases v1.5.2 to v1.5.4 | community-pack 6a, release 3d |
| `contracts/`, `test_contracts.py` | the parser rules of the old releases for each published file (feed, config, hub catalog), checked against the publishers' own output | release 3d, the gate |
| `testkit.py`, `test_*.py` | hermetic tests of the tools above (fakes; no network, no emulator, no gh, no token) | the gate |

Tests (about a second):

    python3 -I -m unittest discover -s tools/release -p 'test_*.py'

## update_check.py

Checks that the previous release updates to the new one (release step 6b), and that the data an older
release left carries over to the new program (the carry-over check, steps 3b and 6b).

After the release (the published binary is the new one):

    $(ai-resources prefix) python3 -I tools/release/update_check.py --from v1.5.1 --to v1.5.2

Before the tag (the locally built program; the carry-over check only, no self-update, no Android):

    $(ai-resources prefix) python3 -I tools/release/update_check.py --from v1.5.1 --from v1.5.0 --from v1.4.1 \
        --new-bin target/release/razdor

The `--from` list comes from `from_list.py --args` (see below).

- `--to` must be the newest published release (drafts and pre-releases aside): the updater only
  offers the newest. `--from` must be an older published release; give it once per old release.
  Each `--from` runs in its own folder under the scratch folder, and the desktop part runs for each.
- The release list is read on every page of the GitHub API (100 at a time): the repo has more than 30
  releases, and an old one such as v1.4.1 is not in the first page.
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

## from_list.py: the `--from` list

    python3 -I tools/release/from_list.py --args                # before the tag: the newest published release and the rest
    python3 -I tools/release/from_list.py --to v1.5.5 --args    # after the release: --to is the new one

Prints the older releases the update check runs against, newest first (`--args`: as one line of
`--from` arguments). The reasons go to stderr; they carry download counts, so they stay private.

- the previous release: the newest published one before `--to` (without `--to`: the newest published);
- the oldest release of each minor line (X.Y) that still had player downloads in the last `--window` days
  (default 30). The count is the growth of the player platforms (windows, linux, macos, android, ios)
  between the last snapshot at or before the window's start and the newest snapshot. The `translate` and
  `sums` rows (bots, the updater's checksum fetch) are not counted. A release published inside the window
  counts from zero; one older than the window with no snapshot before its start counts from its first
  snapshot (a lower bound, flagged in the reason);
- v1.5.0 and v1.4.1 whenever they exist (the owner's floor, 2026-11-22);
- `--min-downloads N` raises the bar (default 1); `--csv` (default `$RAZDOR_STATS_CSV`, else
  `/mnt/ai_models/razdor-stats/downloads.csv`); `--releases FILE` (a JSON list of release objects instead of
  the GitHub API; for tests and offline use).
- Exit 2: no stats CSV (`--allow-no-stats` prints the previous release and the floor only), or no published
  release before the new one.

## android_update_check.py: the Android update

    python3 -I tools/release/android_update_check.py --from v1.5.3 --to v1.5.4
    python3 -I tools/release/android_update_check.py --from v1.5.3 --new-apk <APK> --static-only

1. The APKs (the rules of `update_check.check_apk_pair`): the old APK comes from its release and must match its
   `SHA256SUMS` line; both verify (`apksigner`); the same signing certificate; the same package; a versionCode
   that does not go down; the versionName. Every release so far ships versionCode 1.
2. A private headless emulator: a copy of the AVD (2.4 GB; removed at the end unless `--keep`), a private Xvfb on a
   free display (never the owner's `:0`), `-gpu host` (Mesa is forced on the X server, so the software renderer
   draws and no GPU is used), `-no-window -no-audio -no-snapshot`, pinned to `ai-resources cpus` at nice 19.
3. The old APK is installed (after an uninstall, so the data starts empty), started, and its log
   (`files/Razdor/razdor.log` in the app's folder) names its version. Data is made the way a player's is:
   `settings.json` with a test marker and `saves/carry-test.txt`, pushed with `adb push`. A release APK is not
   debuggable, so `run-as` does not work; `adb push` into the app's folder does.
4. The old app runs 30 s and its crash lines are recorded (log and logcat). The data must read back as pushed,
   after a `sync`, before the emulator stops. The emulator is then booted again: the new version gets a fresh GL
   session (see the emulator notes).
5. The new APK is installed over the old one (`adb install -r`); the package reports the new version; the app
   starts and its log names the new version; it runs 30 s. A crash line that only the new version shows fails
   the check; one that both versions show is an emulator limit (INFO). The saves file must be unchanged; the
   settings file unchanged, or rewritten by the app with the test's marker still set.

Exit: 0 every check passed, 1 a check failed (the install, the version, a new crash, the data), 2 the check
could not run (no SDK, no emulator boot, no network, a device already on the port).

Emulator notes (measured on this machine, 2026-11-22):

- The emulator needs a display even with `-no-window`: it runs on a private Xvfb (the script starts one).
- `-gpu swiftshader_indirect` segfaults (exit -11) just after «Emulator is performing a full startup» here;
  `-gpu host` on the private Xvfb boots. The script forces Mesa (`__GLX_VENDOR_LIBRARY_NAME=mesa`,
  `LIBGL_ALWAYS_SOFTWARE=1`), so no GPU is used.
- A second GL app on the same emulator process failed to create its EGL config (miniquad `egl.rs:209`, the
  `cfg_count > 0` assert): the check restarts the emulator before the new version. A player's update is a new
  process too.
- `adb shell cat` goes through a terminal and turns LF into CRLF: the check reads files with `adb pull`.
- A run takes about 9 minutes (two boots and the two APK downloads): start it in the background with a done-file.

Not covered: a real phone (Play, the installer dialog, the app's own updater, which offers «Ask» on a phone),
a player's saves and Hub, the first-start conversion on a phone (see the mobile checks).

Before the tag, the pipeline's APK is unsigned (the signing job runs after the tag), and an unsigned or
debug-signed APK cannot install over a release-signed one. So the emulator check runs after the release (6c)
on the published APK. Before the tag, only `--static-only --new-apk <APK signed with the release key>` is
possible, and only if the owner has that build here.

Setup, if the SDK, the image or the AVD is missing (on this machine all exist under
`/mnt/ai_models/razdor-scratch/android-apk`; nothing was downloaded for this check):

    sdkmanager "platform-tools" "emulator" "platforms;android-34" "build-tools;35.0.0" \
        "system-images;android-34;google_apis;x86_64"       # the system image is about 4.3 GB
    avdmanager create avd -n razdor -k "system-images;android-34;google_apis;x86_64" -d pixel_6 --force

The AVD here: android-34 google_apis x86_64, 1080x2340, 1536 MB RAM, a 512 MB SD card.

## rollback.py: withdraw a bad release

    python3 -I tools/release/rollback.py --to v1.5.3                # dry run (the default): prints the plan
    python3 -I tools/release/rollback.py --to v1.5.3 --bad v1.5.4 --apply

The updaters read two things: the GitHub release list (drafts and pre-releases skipped; an offer only for a
release newer than the program) and the update feed `releases.json` on the razdor-community Pages (the first
address of `UPDATE_FEED` in `src/mirror.rs`). A rollback makes the bad release a pre-release on GitHub
(`gh release edit vBAD --repo indicozy/razdor --prerelease`) and rewrites the feed without it and without
anything newer than the target. The feed is written through `tools/gitverse/pages.py` (a commit on
razdor-community `main`, then a wait until the address serves the new bytes). The target's own entry keeps its
asset addresses.

- The dry run reads GitHub, the feed and the live config (anonymous reads) and writes nothing. It prints the two
  changes, the warnings and any blockers.
- Blockers (exit 1, nothing changed): the bad release is not the newest published one; the target is not older
  than it or not published; the feed cannot be read or does not list the target; the repo's
  `data/remote/config.json` differs from the live config (a Pages push writes the repo's file over the live one).
- Warnings: the live config names the bad tag (fix `data/remote/config.json`, then `publish_config.py`); its
  `update_feed` does not list the feed; the config cannot be read.
- `--apply` writes the feed first, then flips GitHub, then checks that the newest published release is the target.
  It prints what it changed; a failure says what may have changed.
- A player already on the bad release is not moved back: the updaters never offer an older release. Those players
  get the next fixed release.
- `--apply` needs `gh` logged in as indicozy (`gh auth token -u indicozy`) and `GITVERSE_TOKEN` (in `.env`).

## hub_candidate_check.py: the old releases and a candidate catalog

Serves a candidate hub catalog from a local HTTPS server and runs the old Linux releases against it. Run it
before any catalog publish (community-pack step 6a; release step 3d). Give it the publish dry run's folder:

    python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --dry-run
    python3 -I tools/release/hub_candidate_check.py --catalog-dir <out>

The check reads `<out>/gitverse/index-v2.json` and the files beside it. Nothing is published.

- Every address the index names is rewritten to `https://127.0.0.1:<port>/cand/...`. Each file is matched by
  its size and SHA-256 in the candidate's folders; a wrapped file's zip is checked by the archive fields and
  its member by the file's own size and SHA-256. A missing or wrong file stops the run before anything is launched.
- v1.5.2, v1.5.3 and v1.5.4 (downloaded from their GitHub releases and checked against `SHA256SUMS`) run on a
  private Xvfb with `RAZDOR_HUB` on the candidate, fresh HOME and XDG folders and `updates: off`. Every
  address other than the local server goes to a closed local port. Without an install, a program reads the
  catalog at launch and starts the game download from the catalog's `game` entry by itself. The check passes
  when the log says the catalog was read with a game in it, offers the game at the catalog's size, and ends
  with `installed`, and when the server served the game's bytes.
- Trust: `CURL_CA_BUNDLE` and `SSL_CERT_FILE` point at a self-signed certificate made in the scratch folder.
  The program calls the system curl, which reads `CURL_CA_BUNDLE`, so the programs need no change.
- Exit codes: 0 every version passed; 1 a version or the candidate refused (the log says why); 2 the check
  could not run (input, network, tools).
- Scratch: `--work` (default under `/mnt/ai_models/razdor-scratch/release-checks2/`). Without `--keep` only
  `result.json` is left there.
- Not covered: the Hub window. A map, a campaign and a mod are downloaded by clicking in the Hub, and no
  option starts a Hub download. The check verifies their bytes on the server (size, SHA-256, and the zip
  member's) but does not run the app's own download of them. An app hook would close the gap: an environment
  variable (for example `RAZDOR_HUB_INSTALL=<id>`) that starts a Hub install at launch and logs its result.
  No hook was added: this check changes no app code.

## contracts/: what the old releases read

`contracts/rules.py` has the parser rules that each old release applies to the files it reads: the update feed
(`releases.json`), the remote config (`config.json`, schema 1) and the hub catalog (`index-v2.json`, and the
flat `index.json`). The rules come from the release tags v1.4.1, v1.5.0 and v1.5.2 to v1.5.4; the last three
read the same hub, update and config sources.

`test_contracts.py` makes each file with the publishers' own functions in a temp folder and checks it against
the rules. It also checks that every rule refuses its bad case, and that unknown fields are ignored (no version
denies them). `contracts/samples/` holds made-up files for each format.

Two rules to remember. v1.4.1 and v1.5.0 need the sums asset named exactly `SHA256SUMS`; v1.5.2 also accepts
`SHA256SUMS.txt`. A catalog's `game` must be an https address with a 64-digit SHA-256 and a size up to 256 MB,
or the old programs refuse the whole catalog.

## Tests

`testkit.HermeticTestCase` gives each test a temp folder and makes a socket connection raise. The tests inject
fakes for the release list, the feeds, the stats CSV, the adb device, the emulator process and the Pages writer.
Only the mirror and remote-config source files are read (for their addresses), never written.
