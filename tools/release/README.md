# Release tools

`update_check.py`: checks that the previous release updates to the new one (release step 6b).

    $(ai-resources prefix) python3 -I tools/release/update_check.py --from v1.5.0 --to v1.5.1

- `--to` must be the newest published release (drafts and pre-releases aside): the updater only
  offers the newest. `--from` must be an older published release.
- Needs: network (the GitHub API and downloads, anonymous: the repo is public), `Xvfb` (Linux), the
  Android SDK build-tools with `apksigner` and `aapt2` (`ANDROID_HOME` or `ANDROID_SDK_ROOT`; else
  `/mnt/ai_models/razdor-scratch/android-apk/sdk`), and Java for `apksigner`. `RAZDOR_DT_DIR` comes
  from the environment or the repo's `.env`; it is never printed.
- Scratch folder: `--scratch`, else `$RAZDOR_SCRATCH`, else `/mnt/ai_models/razdor-scratch/update-check/<from>-<to>`,
  else a new private temp folder. It holds downloaded programs, so it must be a real folder owned by
  this user and not writable by group or others (the tool refuses a symlink or a looser folder, and
  sets it to 0700). It is removed on success unless `--keep`; the processes the tool starts are always stopped.
- Use `$(ai-resources prefix)` (CPUs 8-19 at nice 19 on the desk): the game renders in software under Xvfb.
- Exit: 0 all checks pass (INFO rows are not failures), 1 a check failed, 2 the check could not run.
- Not covered: Windows, macOS, iOS, and a player's real install (its saves, the Hub, the phone's
  install dialog).
