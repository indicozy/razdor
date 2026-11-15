# Release tools

`update_check.py`: checks that the previous release updates to the new one (release step 6b).

    ANDROID_HOME=<sdk> python3 -I tools/release/update_check.py --from v1.5.0 --to v1.5.1

- `--to` must be the newest published release (drafts and pre-releases aside): the updater only
  offers the newest. `--from` must be an older published release.
- Needs: network (the GitHub API and downloads, no token), `Xvfb` (Linux), the Android SDK
  build-tools with `apksigner` and `aapt2` (`ANDROID_HOME`, or `ANDROID_SDK_ROOT`), and Java for
  `apksigner`. `RAZDOR_DT_DIR` comes from the environment or the repo's `.env`; it is never printed.
- `--scratch DIR` (or `$RAZDOR_SCRATCH`) holds the downloads and the game's saves and logs; it is
  removed on success unless `--keep`, and the processes it starts are always stopped.
- Exit: 0 all checks pass (INFO rows are not failures), 1 a check failed, 2 the check could not run.
- Not covered: Windows, macOS, iOS, and a player's real install (its saves, the Hub, the phone's
  install dialog).
