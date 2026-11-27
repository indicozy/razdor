//! Decodes DT picture files and writes each frame as raw RGBA, for the website's art pack
//! (tools/site/art_pack.py, the `lit` entries).
//!
//! Usage: `dump_rgba <out dir> <file>...`. For every frame `i` of `<file>` the output is
//! `<out dir>/<file stem>-<i>.rgba`: a u32 LE width, a u32 LE height, then width × height × 4 bytes
//! of RGBA.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: dump_rgba <out dir> <file>...");
        return ExitCode::from(2);
    }
    let out = Path::new(&args[1]);
    if let Err(e) = fs::create_dir_all(out) {
        eprintln!("crit: cannot create {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    for p in &args[2..] {
        let path = Path::new(p);
        let frames = match razdor::dt::gfx::decode_file(path) {
            Ok(frames) => frames,
            Err(e) => {
                eprintln!("crit: cannot decode {p}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            eprintln!("crit: no file name in {p}");
            return ExitCode::FAILURE;
        };
        for (i, im) in frames.iter().enumerate() {
            let mut buf = Vec::with_capacity(8 + im.rgba.len());
            buf.extend_from_slice(&im.width.to_le_bytes());
            buf.extend_from_slice(&im.height.to_le_bytes());
            buf.extend_from_slice(&im.rgba);
            let target = out.join(format!("{stem}-{i}.rgba"));
            if let Err(e) = fs::write(&target, buf) {
                eprintln!("crit: cannot write {}: {e}", target.display());
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
