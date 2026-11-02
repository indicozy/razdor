//! `razdor-pack`: checks content packages (`.rzpack`, or their folder) and builds the `dt:`
//! alias table from the install. Razdor's library (`razdor::pack`); no window.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use razdor::pack::{self, Files, Package};

const HELP: &str = "razdor-pack: content packages (.rzpack) for Razdor 2.0

  razdor-pack check <folder-or-.rzpack> [--json]
                             check a package against the format's rules
                             (docs/reference/rzpack-format.md): errors and warnings as
                             file:line with a hint; --json for agents and tools
                             exit status: 0 no errors, 1 errors, 2 usage or read problem
  razdor-pack gen-aliases [--out <path>]
                             write the dt: alias table of the install (RAZDOR_DT_DIR, or
                             the one Razdor finds) with Razdor's English names
                             (default data/pack/dt-aliases.toml)
  --help                     this text

Later commands, not built yet: render (pictures of a package), sim (the AI plays it
headless), play (opens it in Razdor), pack (folder to .rzpack).";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("razdor-pack: {message}");
            eprintln!("(razdor-pack --help)");
            ExitCode::from(2)
        }
    }
}

/// The exit status, or a usage or read problem.
fn run(args: &[String]) -> Result<u8, String> {
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => {
            println!("{HELP}");
            Ok(0)
        }
        Some("check") => check(&args[1..]),
        Some("gen-aliases") => gen_aliases(&args[1..]),
        Some(other) => Err(format!("unknown command `{other}`")),
    }
}

fn check(args: &[String]) -> Result<u8, String> {
    let mut json = false;
    let mut path: Option<PathBuf> = None;
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}` for check")),
            p if path.is_none() => path = Some(PathBuf::from(p)),
            _ => return Err("check takes one folder or .rzpack file".into()),
        }
    }
    let path = path.ok_or("check needs a folder or a .rzpack file")?;
    let files = Files::load(&path)?;
    let (package, _) = Package::parse(files);
    let report = pack::check(&package);
    if json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(if report.has_errors() { 1 } else { 0 })
}

fn gen_aliases(args: &[String]) -> Result<u8, String> {
    let mut out = PathBuf::from("data/pack/dt-aliases.toml");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = PathBuf::from(it.next().ok_or("--out needs a path")?),
            other => return Err(format!("unknown option `{other}` for gen-aliases")),
        }
    }
    razdor::dt::install::load_dotenv();
    let dir = razdor::dt::install::locate().ok_or("no Discord Times install found (set RAZDOR_DT_DIR)")?;
    let dt = razdor::dt::install::DtInstall::load(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if dt.english.is_none() {
        return Err("the English files were not found (Razdor's copies are missing from the build)".into());
    }
    let generated = pack::aliases::generate(&dt);
    write(&out, &generated.text)?;
    let [units, items, spells] = generated.counts;
    println!(
        "wrote {}: {units} units, {items} items, {spells} spells; {} without an English name",
        out.display(),
        generated.unnamed
    );
    Ok(0)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}
