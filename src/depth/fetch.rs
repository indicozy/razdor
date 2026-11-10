//! The depth model file: where it lives, its size and SHA-256, and its download. The model is
//! never fetched silently: the Settings row starts the download (`start`), once. The file is
//! checked by size and SHA-256 before it takes its place; a file that fails the check is removed
//! and the download is reported as failed (the row offers it again).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::platform;
use crate::update;

/// The release asset: Depth Anything V2 Small as the engine loads it (`tools/depth/README.md`).
/// Uploaded to the community repository's `depth-model-v1` release with the owner's yes.
pub const DEPTH_MODEL_URL: &str = "https://github.com/indicozy/razdor-community/releases/download/depth-model-v1/depth-anything-v2-small-fp16-266.onnx";
/// The file's name, in the models folder.
pub const DEPTH_MODEL_FILE: &str = "depth-anything-v2-small-fp16-266.onnx";
/// The file's size in bytes.
pub const DEPTH_MODEL_SIZE: u64 = 49_562_985;
/// The file's SHA-256, lowercase hex.
pub const DEPTH_MODEL_SHA256: &str = "897f1b8788fdab97f0e75155e4fdc9a8e7605734f1cd31469a4ca6fb02ea8e74";
/// The variable that names a model file to use as it is (development and tests: no download).
pub const ENV_VAR: &str = "RAZDOR_DEPTH_MODEL";
/// The most a download may take, in seconds (a slow line still gets the 50 MB through).
const SECONDS: u32 = 900;

/// Where the model's download stands, as the Settings row shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// No model, none downloading: the row offers the download.
    Absent,
    /// Downloading, with the percentage so far.
    Downloading(u8),
    /// The last download failed (the row offers it again).
    Failed,
    /// The model is there.
    Installed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Downloading(u8),
    Failed,
}

static PHASE: Mutex<Phase> = Mutex::new(Phase::Idle);

fn set_phase(phase: Phase) {
    *PHASE.lock().unwrap_or_else(|e| e.into_inner()) = phase;
}

/// The folder the model is kept in: the app's folder on the desktop, the app's cache folder on the
/// phones (the player can clear it; the row offers the download again). `None` where there is no
/// such folder.
pub fn models_dir() -> Option<PathBuf> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        platform::cache_dir().map(|d| d.join("models"))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        platform::app_dir().map(|d| d.join("models"))
    }
}

/// The model file in use: the file the variable names, or the downloaded one when its size is
/// right. `None` when there is no model.
pub fn model_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_VAR) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let path = models_dir()?.join(DEPTH_MODEL_FILE);
    (std::fs::metadata(&path).ok()?.len() == DEPTH_MODEL_SIZE).then_some(path)
}

/// The download's state for the Settings row.
pub fn status() -> Status {
    if model_file().is_some() {
        return Status::Installed;
    }
    match PHASE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        Phase::Idle => Status::Absent,
        Phase::Downloading(p) => Status::Downloading(p),
        Phase::Failed => Status::Failed,
    }
}

/// Starts the download on a thread of its own. Does nothing while a download runs or when the
/// model is there; a failed one is tried again.
pub fn start() {
    if model_file().is_some() {
        return;
    }
    {
        let mut phase = PHASE.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*phase, Phase::Downloading(_)) {
            return;
        }
        *phase = Phase::Downloading(0);
    }
    let spawned = std::thread::Builder::new().name("depth-download".into()).spawn(|| {
        let phase = match models_dir().ok_or_else(|| "no folder for the model".to_string()).and_then(|dir| download(&dir)) {
            Ok(()) => Phase::Idle,
            Err(e) => {
                crate::diag!("depth model download failed: {e}");
                Phase::Failed
            }
        };
        set_phase(phase);
    });
    if spawned.is_err() {
        set_phase(Phase::Failed);
    }
}

/// Downloads the model into `dir` and checks it. Its progress goes to [`Status::Downloading`].
fn download(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dest = dir.join(DEPTH_MODEL_FILE);
    let part = dir.join(format!("{DEPTH_MODEL_FILE}.part"));
    fetch_verified(&part, &dest, DEPTH_MODEL_SIZE, DEPTH_MODEL_SHA256, |path| {
        let progress = |n: u64| {
            let percent = ((n * 100) / DEPTH_MODEL_SIZE).min(99) as u8;
            let mut phase = PHASE.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*phase, Phase::Downloading(_)) {
                *phase = Phase::Downloading(percent);
            }
        };
        let code = update::download_watched(DEPTH_MODEL_URL, path, 0, SECONDS, &progress, &|| false)?;
        if code != 200 {
            return Err(format!("HTTP {code}"));
        }
        Ok(())
    })
}

/// Runs `get` into `part`, checks the file's size and SHA-256 against the given ones, and renames
/// it to `dest`. A failed transfer or a failed check removes `part` and gives the reason; nothing
/// is left at `dest` by this call.
pub fn fetch_verified(part: &Path, dest: &Path, size: u64, sha256: &str, get: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let _ = std::fs::remove_file(part);
    let result = get(part)
        .and_then(|()| check(part, size, sha256))
        .and_then(|()| std::fs::rename(part, dest).map_err(|e| format!("{}: {e}", dest.display())));
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
}

/// Checks a file's size and SHA-256 (lowercase hex).
pub fn check(path: &Path, size: u64, sha256: &str) -> Result<(), String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if len != size {
        return Err(format!("{len} bytes, not {size}"));
    }
    let hash = update::sha256_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if hash != sha256 {
        return Err(format!("the SHA-256 is {hash}, not {sha256}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp dir, removed by the test.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let p = std::env::temp_dir().join(format!("razdor-depth-fetch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A "download" that copies a local file, as the network would hand it over.
    fn copy_from(src: PathBuf) -> impl FnOnce(&Path) -> Result<(), String> {
        move |part| std::fs::copy(&src, part).map(|_| ()).map_err(|e| e.to_string())
    }

    #[test]
    fn a_file_of_the_right_size_and_hash_takes_its_place() {
        let dir = Dir::new("ok");
        let src = dir.0.join("src.bin");
        std::fs::write(&src, b"model bytes").unwrap();
        let (part, dest) = (dir.0.join("m.part"), dir.0.join("m.onnx"));
        let sha = crate::update::sha256_hex(b"model bytes");
        fetch_verified(&part, &dest, 11, &sha, copy_from(src)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"model bytes");
        assert!(!part.exists(), "the partial file is renamed, not kept");
    }

    #[test]
    fn a_size_mismatch_is_refused_and_nothing_is_kept() {
        let dir = Dir::new("size");
        let src = dir.0.join("src.bin");
        std::fs::write(&src, b"model bytes").unwrap();
        let (part, dest) = (dir.0.join("m.part"), dir.0.join("m.onnx"));
        let sha = crate::update::sha256_hex(b"model bytes");
        let err = fetch_verified(&part, &dest, 12, &sha, copy_from(src)).unwrap_err();
        assert!(err.contains("11 bytes, not 12"), "{err}");
        assert!(!part.exists() && !dest.exists());
    }

    #[test]
    fn a_hash_mismatch_is_refused_and_nothing_is_kept() {
        let dir = Dir::new("hash");
        let src = dir.0.join("src.bin");
        std::fs::write(&src, b"model bytes").unwrap();
        let (part, dest) = (dir.0.join("m.part"), dir.0.join("m.onnx"));
        let wrong = crate::update::sha256_hex(b"other bytes");
        let err = fetch_verified(&part, &dest, 11, &wrong, copy_from(src)).unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert!(!part.exists() && !dest.exists());
    }

    #[test]
    fn a_failed_transfer_leaves_no_partial_file() {
        let dir = Dir::new("transfer");
        let (part, dest) = (dir.0.join("m.part"), dir.0.join("m.onnx"));
        let err = fetch_verified(&part, &dest, 11, "00", |path| {
            std::fs::write(path, b"half")
                .map_err(|e| e.to_string())?;
            Err("HTTP 404".to_string())
        })
        .unwrap_err();
        assert_eq!(err, "HTTP 404");
        assert!(!part.exists() && !dest.exists());
    }
}
