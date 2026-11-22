//! `razdor-verify`: replays one battle with the install's own engine and prints the verdict as one
//! JSON line (`{"verdict": "ok" | "suspicious" | "unverified", "reason": ...}`). The battle is the
//! file `GET /v1/battles/{id}` returns (the metadata, then the record, framed). The install is
//! `RAZDOR_DT_DIR` (from `.env`). Design: `docs/superpowers/specs/2026-11-22-analytics-design.md`.
//!
//! Usage: `razdor-verify <battle file>`. Exit 0 with a verdict; exit 2 when the file or the install
//! cannot be read at all (no verdict).

use std::process::ExitCode;
use std::sync::Arc;

use razdor::dt::install::{load_dotenv, DtInstall, ENV_VAR};
use razdor::rules::content::Content;
use razdor::telemetry::verify::verify;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: razdor-verify <battle file>");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read the battle: {e}");
            return ExitCode::from(2);
        }
    };
    load_dotenv();
    let Some(dir) = std::env::var_os(ENV_VAR) else {
        eprintln!("{ENV_VAR} is not set: the verifier needs the install");
        return ExitCode::from(2);
    };
    let dt = match DtInstall::load(std::path::Path::new(&dir)) {
        Ok(dt) => dt,
        Err(e) => {
            eprintln!("cannot load the install: {e:?}");
            return ExitCode::from(2);
        }
    };
    let content = Arc::new(Content::from_dt(&dt));
    println!("{}", verify(&bytes, content).to_json());
    ExitCode::SUCCESS
}
