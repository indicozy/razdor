//! `razdor-dtm-json`: writes one `.DTm` map as JSON (format `razdor-dtm-json`, version 1; the
//! layout is in `razdor::dt::dtm_json` and `docs/flows/testing-and-tools.md`).
//!
//! Usage: `razdor-dtm-json <map.DTm> [-o <out.json>] [--compact]`. Without `-o` the pretty JSON
//! goes to stdout. It reads only the map file: no install and no `.env` are needed. Errors go to
//! stderr and exit with code 2.

use std::fmt::Display;
use std::path::PathBuf;
use std::process::ExitCode;

use razdor::dt::dtm::Scenario;
use razdor::dt::dtm_json;

const USAGE: &str = "usage: razdor-dtm-json <map.DTm> [-o <out.json>] [--compact]";

fn fail(msg: impl Display) -> ExitCode {
    eprintln!("razdor-dtm-json: {msg}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut compact = false;
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
            flag if flag.starts_with('-') => return fail(format!("unknown option {flag}\n{USAGE}")),
            _ => {
                if input.replace(PathBuf::from(&arg)).is_some() {
                    return fail(format!("only one map at a time\n{USAGE}"));
                }
            }
        }
    }
    let Some(input) = input else { return fail(USAGE) };
    let scenario = match Scenario::load(&input) {
        Ok(s) => s,
        Err(e) => return fail(format!("cannot read {}: {e}", input.display())),
    };
    let source_file = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let json = match dtm_json::scenario_to_string(&scenario, &source_file, !compact) {
        Ok(json) => json,
        Err(e) => return fail(format!("cannot write the JSON: {e}")),
    };
    match out {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, format!("{json}\n")) {
                return fail(format!("cannot write {}: {e}", path.display()));
            }
        }
        None => println!("{json}"),
    }
    ExitCode::SUCCESS
}
