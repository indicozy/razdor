#!/usr/bin/env python3
"""Writes data/game-manifest.json from the community's game archive (the GitHub .zip, `discord-times-cu.zip`).

The manifest holds only what the integrity check needs: the archive's name, size and SHA-256, its
top folder, and for every file under that folder its path, size and SHA-256. No file content is
written. Run it on the archive that `RAZDOR_GDRIVE_E2E_DIR` download produced (the real file):

    python3 -I scripts/game-manifest.py <archive.zip> data/game-manifest.json

The archive must be the one `src/gdrive.rs` (GAME) names: its size and SHA-256 are checked first.
"""

import hashlib
import json
import os
import sys
import zipfile

ARCHIVE_NAME = "Discord Times Community Update (Unstable).zip"
ARCHIVE_SIZE = 46_726_043
ARCHIVE_SHA256 = "fce6d8aa22f8932cddf0dffaf02d3c013d15b298148fc2c50154306d24f9f37e"
TOP = "Discord Times Community Update (Unstable)"


def sha256_of(stream):
    digest = hashlib.sha256()
    for block in iter(lambda: stream.read(1 << 20), b""):
        digest.update(block)
    return digest.hexdigest()


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    archive, out = sys.argv[1], sys.argv[2]
    size = os.path.getsize(archive)
    with open(archive, "rb") as f:
        sha = sha256_of(f)
    if size != ARCHIVE_SIZE or sha != ARCHIVE_SHA256:
        sys.exit(f"not the community archive: {size} bytes, sha256 {sha}")

    files = []
    with zipfile.ZipFile(archive) as zf:
        for info in zf.infolist():
            if info.is_dir():
                continue
            if not info.filename.startswith(TOP + "/"):
                sys.exit(f"an entry outside the top folder: {info.filename}")
            rel = info.filename[len(TOP) + 1 :]
            # zipfile checks each entry's CRC while it reads it: a damaged archive stops here.
            with zf.open(info) as entry:
                digest = sha256_of(entry)
            files.append({"path": rel, "size": info.file_size, "sha256": digest})
    files.sort(key=lambda e: e["path"])
    manifest = {
        "archive": {"name": ARCHIVE_NAME, "size": size, "sha256": sha, "top": TOP},
        "files": files,
    }
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=1)
        f.write("\n")
    print(f"{len(files)} files written to {out}")


if __name__ == "__main__":
    main()
