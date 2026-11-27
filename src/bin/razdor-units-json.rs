//! `razdor-units-json`: writes the install's unit cards as JSON (format `razdor-units-json`, version 2;
//! the layout is in `razdor::dt::units_json` and `docs/flows/testing-and-tools.md`).
//!
//! Usage: `razdor-units-json [-o <out.json>] [--compact] [--lang <prefix>]`. The install is
//! `RAZDOR_DT_DIR` (from `.env`). Without `-o` the pretty JSON goes to stdout. `--lang Eng` writes
//! the names in the English translation (`Eng_*.ini`); the default is the install's Russian.
//! Exit 0 when written; exit 2 when the install is missing, cannot be loaded or the usage is wrong.
//! The output is game data: write it outside the repo.

use std::fmt::Display;
use std::path::PathBuf;
use std::process::ExitCode;

use razdor::dt::install::{load_dotenv, DtInstall, ENV_VAR};
use razdor::dt::units_json;
use razdor::rules::content::Content;

const USAGE: &str = "usage: razdor-units-json [-o <out.json>] [--compact] [--lang <prefix>]";

fn fail(msg: impl Display) -> ExitCode {
    eprintln!("razdor-units-json: {msg}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut out: Option<PathBuf> = None;
    let mut compact = false;
    let mut lang: Option<String> = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        match text.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--compact" => compact = true,
            "-o" => match args.next() {
                Some(path) => out = Some(PathBuf::from(path)),
                None => return fail("-o needs a file name"),
            },
            "--lang" => match args.next() {
                Some(prefix) => lang = Some(prefix.to_string_lossy().into_owned()),
                None => return fail("--lang needs a data language prefix, such as Eng"),
            },
            _ => return fail(format!("unknown argument {text}\n{USAGE}")),
        }
    }

    load_dotenv();
    let Some(dir) = std::env::var_os(ENV_VAR) else {
        return fail(format!("{ENV_VAR} is not set: the exporter needs the install (.env)"));
    };
    let dt = match DtInstall::load(std::path::Path::new(&dir)) {
        Ok(dt) => dt,
        Err(e) => return fail(format!("cannot load the install: {e:?}")),
    };
    let content = Content::from_dt_in(&dt, lang.as_deref());
    let language = lang.as_deref().unwrap_or("ru");
    let json = match units_json::export_to_string(&content.units, &content.spells, language, !compact) {
        Ok(json) => json,
        Err(e) => return fail(format!("cannot write the JSON: {e}")),
    };
    match out {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, format!("{json}\n")) {
                return fail(format!("cannot write {}: {e}", path.display()));
            }
            eprintln!("razdor-units-json: {} units and {} spells to {}", content.units.len(), content.spells.len(), path.display());
        }
        None => println!("{json}"),
    }
    ExitCode::SUCCESS
}
