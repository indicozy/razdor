//! `razdor-mirror`: pulls the owner's public release files from GitHub into the download mirror (the web root
//! nginx serves at `/hub/`). `razdor-mirror [--config FILE]` (default `/etc/razdor-mirror.toml`). Started by
//! `razdor-mirror-sync.service` every 15 minutes and on demand. Exit 0: synced, or nothing to do (another run
//! holds the lock, or GitHub's rate limit); exit 1: something failed (the log says what).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use razdor_server::mirror::config::MirrorConfig;
use razdor_server::mirror::net::Http;
use razdor_server::mirror::{self, store};

fn usage() -> ExitCode {
    eprintln!("usage: razdor-mirror [--config FILE]");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::from("/etc/razdor-mirror.toml");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => match args.next() {
                Some(p) => path = PathBuf::from(p),
                None => return usage(),
            },
            "-h" | "--help" => {
                println!("usage: razdor-mirror [--config FILE]");
                return ExitCode::SUCCESS;
            }
            _ => return usage(),
        }
    }

    let cfg = match MirrorConfig::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("crit: config: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&cfg.stage) {
        eprintln!("crit: stage {}: {e}", cfg.stage.display());
        return ExitCode::FAILURE;
    }
    // One run at a time: a second start (the timer, an on-demand trigger) finds the lock and stops.
    let _lock = match store::lock(&cfg.stage.join(".lock")) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            eprintln!("info: another run holds the lock; nothing to do");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("crit: lock: {e}");
            return ExitCode::FAILURE;
        }
    };

    match mirror::run(&cfg, &Http::new(), SystemTime::now()) {
        Err(e) => {
            eprintln!("crit: {e}");
            ExitCode::FAILURE
        }
        Ok(out) if out.rate_limited => {
            eprintln!("info: GitHub's rate limit; ending quietly, the next run tries again");
            ExitCode::SUCCESS
        }
        Ok(out) => {
            for e in &out.errors {
                eprintln!("crit: {e}");
            }
            eprintln!("info: app {:?}, catalog {:?}, GitHub API calls {}", out.app, out.catalog, out.api_calls);
            if out.errors.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
