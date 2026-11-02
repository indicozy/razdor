//! Build script: records the commit for the log, and lets the audio backend link on Linux
//! systems without ALSA's development package.
//!
//! quad-snd links `-lasound`, which needs the development symlink `libasound.so`. Many
//! desktops only have the runtime library `libasound.so.2`; then a symlink to it is made in
//! `OUT_DIR` (inside `target/`) and added to the link search path. Nothing outside `target/`
//! is touched.

use std::path::{Path, PathBuf};

const LIB_DIRS: &[&str] = &[
    "/usr/lib64",
    "/usr/lib",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/lib64",
    "/lib/x86_64-linux-gnu",
    "/lib/aarch64-linux-gnu",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // The commit, for the log (`razdor::diag`).
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    let git = |args: &[&str]| {
        std::process::Command::new("git").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    if let Some(hash) = git(&["rev-parse", "--short", "HEAD"]) {
        let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
        println!("cargo:rustc-env=RAZDOR_GIT={hash}{}", if dirty { "-dirty" } else { "" });
    }
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        windows_resources();
    }
    let audio = std::env::var_os("CARGO_FEATURE_AUDIO").is_some();
    let linux = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux");
    if !audio || !linux {
        return;
    }
    if LIB_DIRS.iter().any(|d| Path::new(d).join("libasound.so").exists()) {
        return;
    }
    let Some(runtime) = LIB_DIRS.iter().map(|d| Path::new(d).join("libasound.so.2")).find(|p| p.exists()) else {
        println!("cargo:warning=libasound not found: install ALSA (alsa-lib) or build with --no-default-features");
        return;
    };
    let dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("alsa-link");
    let link = dir.join("libasound.so");
    if std::fs::create_dir_all(&dir).is_ok() && (link.exists() || symlink(&runtime, &link)) {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
}

/// Razdor.exe's icon (`data/icon/razdor.ico`, made by `scripts/icon.py`) and its version
/// information, compiled by the toolchain's resource compiler: `$WINDRES` if set, else
/// `<arch>-w64-mingw32-windres` (llvm-mingw's llvm-windres, on the PATH in `scripts/dist.sh`),
/// and linked into the program as an object file. Without the compiler the exe is built
/// without them, with a warning.
fn windows_resources() {
    println!("cargo:rerun-if-changed=data/icon/razdor.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let ico = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/icon/razdor.ico");
    let version = env!("CARGO_PKG_VERSION");
    let mut nums: Vec<u32> = version.split(['.', '-']).filter_map(|p| p.parse().ok()).collect();
    nums.resize(4, 0);
    let dotted = nums.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    // The Russian name as a wide string of \x escapes: llvm-windres reads the file in the
    // ANSI code page whatever #pragma code_page says. The icon path with forward slashes.
    let name: String = "Раздор".encode_utf16().map(|u| format!("\\x{u:04x}")).collect();
    let rc = format!(
        "1 ICON \"{ico}\"\n\
         1 VERSIONINFO\n\
         FILEVERSION {dotted}\n\
         PRODUCTVERSION {dotted}\n\
         FILEOS 0x40004\n\
         FILETYPE 0x1\n\
         BEGIN\n\
           BLOCK \"StringFileInfo\"\n\
           BEGIN\n\
             BLOCK \"041904B0\"\n\
             BEGIN\n\
               VALUE \"ProductName\", L\"{name}\"\n\
               VALUE \"FileDescription\", L\"{name}\"\n\
               VALUE \"FileVersion\", \"{version}\"\n\
               VALUE \"ProductVersion\", \"{version}\"\n\
               VALUE \"OriginalFilename\", \"Razdor.exe\"\n\
               VALUE \"LegalCopyright\", \"Copyright (C) 2026 indicozy. GPL-3.0-only\"\n\
             END\n\
           END\n\
           BLOCK \"VarFileInfo\"\n\
           BEGIN\n\
             VALUE \"Translation\", 0x419, 1200\n\
           END\n\
         END\n",
        ico = ico.display().to_string().replace('\\', "/"),
    );
    let rc_path = out.join("razdor.rc");
    let obj = out.join("razdor-res.o");
    if std::fs::write(&rc_path, rc).is_err() {
        return;
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let tool = std::env::var("WINDRES").unwrap_or_else(|_| format!("{arch}-w64-mingw32-windres"));
    let ok = std::process::Command::new(&tool)
        .args(["--input-format=rc", "--output-format=coff", "-i"])
        .arg(&rc_path)
        .arg("-o")
        .arg(&obj)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!("cargo:rustc-link-arg-bins={}", obj.display());
    } else {
        println!("cargo:warning={tool} not found or failed: Razdor.exe is built without its icon");
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(not(unix))]
fn symlink(_: &Path, _: &Path) -> bool {
    false
}
