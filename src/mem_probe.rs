//! Memory for the performance log: the process's resident set (Linux and Android, from `/proc`) and an
//! estimate of the GPU textures Razdor holds, by name. The creating code records each texture it makes
//! with [`set_texture`] (its size in bytes, 0 when it is gone); nothing is read back from the GPU.
//! Logging only: the registry is written beside the creation and never changes what is drawn.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// The estimated texture bytes by name (a name is one creating place; a re-made texture replaces its entry).
static TEXTURES: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

/// Records the bytes of the texture `name` (its size now); 0 removes the entry.
pub fn set_texture(name: &'static str, bytes: u64) {
    let mut map = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    if bytes == 0 {
        map.remove(name);
    } else {
        map.insert(name, bytes);
    }
}

/// The bytes of a `w`×`h` texture of `bytes_per_px` bytes a pixel (RGBA8 is 4).
pub fn texture_bytes(w: u32, h: u32, bytes_per_px: u32) -> u64 {
    u64::from(w) * u64::from(h) * u64::from(bytes_per_px)
}

/// The sum of the recorded texture bytes.
pub fn textures_total() -> u64 {
    TEXTURES.lock().unwrap_or_else(|e| e.into_inner()).values().sum()
}

/// The `n` largest recorded textures, by name, the largest first (ties by name).
pub fn largest(n: usize) -> Vec<(&'static str, u64)> {
    let map = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let mut all: Vec<(&'static str, u64)> = map.iter().map(|(k, v)| (*k, *v)).collect();
    all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    all.truncate(n);
    all
}

/// The VmRSS of a `/proc/<pid>/status` text, in kB; None when the line is missing.
pub fn rss_kb_from_status(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// The process's resident set in MB (Linux and Android), or None where `/proc` has none.
pub fn rss_mb() -> Option<f32> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    rss_kb_from_status(&text).map(|kb| kb as f32 / 1024.0)
}

/// The texture estimate's part of the `mem:` line: the total in MB, then the `n` largest by name.
pub fn texture_text(n: usize) -> String {
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    let top: Vec<String> = largest(n).into_iter().map(|(name, bytes)| format!("{name}:{:.1}", mb(bytes))).collect();
    format!("gpu_tex_est_mb={:.1} top=[{}]", mb(textures_total()), top.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rss_is_read_from_the_vmrss_line_in_kb() {
        let status = "Name:\tRazdor\nVmPeak:\t 900 kB\nVmRSS:\t  123456 kB\nThreads:\t9\n";
        assert_eq!(rss_kb_from_status(status), Some(123_456));
        assert_eq!(rss_kb_from_status("Name:\tx\n"), None, "no VmRSS line, no value");
        assert_eq!(rss_kb_from_status("VmRSS:\tjunk kB\n"), None);
    }

    #[test]
    fn a_texture_is_its_pixels_times_its_bytes_a_pixel() {
        assert_eq!(texture_bytes(1024, 512, 4), 2 * 1024 * 1024);
        assert_eq!(texture_bytes(0, 512, 4), 0);
    }

    #[test]
    fn the_registry_keeps_the_current_size_of_each_texture_and_names_the_largest() {
        set_texture("test-mem-a", 3_000);
        set_texture("test-mem-b", 9_000);
        set_texture("test-mem-c", 1_000);
        let top = largest(2);
        assert_eq!(top.iter().filter(|(n, _)| n.starts_with("test-mem-")).map(|(n, _)| *n).collect::<Vec<_>>(), vec!["test-mem-b", "test-mem-a"]);
        set_texture("test-mem-b", 500);
        assert!(largest(usize::MAX).iter().any(|(n, b)| *n == "test-mem-b" && *b == 500), "a re-made texture replaces its entry");
        set_texture("test-mem-a", 0);
        set_texture("test-mem-b", 0);
        set_texture("test-mem-c", 0);
        assert!(!largest(usize::MAX).iter().any(|(n, _)| n.starts_with("test-mem-")), "0 removes the entry");
    }
}
