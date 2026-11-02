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
  razdor-pack sim <folder-or-.rzpack> [--map <id>] [--days <n>=30] [--json]
                             the AI plays a map headless for n days, the community map
                             smoke test's way (RAZDOR_DT_DIR): a panic, a softlock or a
                             check error fails; reports the day of victory or defeat.
                             Without --map every map of the package is played.
                             exit status: 0 all played, 1 a panic, a softlock or errors
  razdor-pack play <folder-or-.rzpack> [--map <id>]
                             opens the package's map in Razdor (`razdor --pack`)
  razdor-pack gen-aliases [--out <path>]
                             write the dt: alias table of the install (RAZDOR_DT_DIR, or
                             the one Razdor finds) with Razdor's English names
                             (default data/pack/dt-aliases.toml)
  --help                     this text

Not built yet: render (pictures of a package), pack (folder to .rzpack).";

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
        Some("sim") => sim(&args[1..]),
        Some("play") => play(&args[1..]),
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

/// The arguments of `sim` and `play`: a package, `--map <id>`, `--days <n>`, `--json`.
#[derive(Default)]
struct Args {
    path: Option<PathBuf>,
    map: Option<String>,
    days: Option<u64>,
    json: bool,
}

fn package_args(cmd: &str, args: &[String], allow_sim: bool) -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--map" => out.map = Some(it.next().ok_or("--map needs a map id")?.clone()),
            "--days" if allow_sim => {
                let v = it.next().ok_or("--days needs a number")?;
                out.days = Some(v.parse().map_err(|_| format!("--days {v}: a whole number of days"))?);
            }
            "--json" if allow_sim => out.json = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}` for {cmd}")),
            p if out.path.is_none() => out.path = Some(PathBuf::from(p)),
            _ => return Err(format!("{cmd} takes one folder or .rzpack file")),
        }
    }
    if out.path.is_none() {
        return Err(format!("{cmd} needs a folder or a .rzpack file"));
    }
    Ok(out)
}

/// The map ids of a package, sorted (the order `--map` defaults to).
fn map_ids(pkg: &Package) -> Vec<String> {
    pkg.maps.keys().cloned().collect()
}

fn sim(args: &[String]) -> Result<u8, String> {
    let a = package_args("sim", args, true)?;
    let path = a.path.clone().unwrap_or_default();
    let files = Files::load(&path)?;
    let (package, _) = Package::parse(files);
    let report = pack::check(&package);
    if report.has_errors() {
        print!("{}", report.to_text());
        return Ok(1);
    }
    let maps = match &a.map {
        Some(m) => vec![m.clone()],
        None => map_ids(&package),
    };
    if maps.is_empty() {
        return Err("the package has no map (maps/<id>/map.toml)".into());
    }
    razdor::dt::install::load_dotenv();
    let dir = razdor::dt::install::locate().ok_or("no Discord Times install found (set RAZDOR_DT_DIR)")?;
    let install = razdor::dt::install::DtInstall::load(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let days = a.days.unwrap_or(30);
    let mut failed = false;
    let mut json = Vec::new();
    for map in &maps {
        match pack::sim::sim_map(&package, &install, map, days) {
            Ok(result) => {
                failed |= !result.ok();
                if a.json {
                    json.push(result.json());
                } else {
                    println!("{}", result.text());
                }
            }
            Err(report) => {
                failed = true;
                if a.json {
                    json.push(serde_json::json!({"map": map, "ok": false, "errors": report.to_text()}));
                } else {
                    print!("{}", report.to_text());
                }
            }
        }
    }
    if a.json {
        println!("{}", serde_json::Value::Array(json));
    }
    Ok(if failed { 1 } else { 0 })
}

/// Opens the package's map in Razdor: `razdor --pack <path> --map <id>`, the binary next to this one.
fn play(args: &[String]) -> Result<u8, String> {
    let a = package_args("play", args, false)?;
    let path = a.path.clone().unwrap_or_default();
    let files = Files::load(&path)?;
    let (package, _) = Package::parse(files);
    let report = pack::check(&package);
    if report.has_errors() {
        print!("{}", report.to_text());
        return Ok(1);
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find razdor-pack: {e}"))?;
    let razdor = exe.with_file_name(if cfg!(windows) { "razdor.exe" } else { "razdor" });
    let mut cmd = if razdor.exists() {
        std::process::Command::new(razdor)
    } else {
        return Err(format!(
            "no razdor next to {} (build it: cargo build --release --bin razdor; or run cargo run --release --bin razdor -- --pack {})",
            exe.display(),
            path.display()
        ));
    };
    cmd.arg("--pack").arg(&path);
    if let Some(m) = &a.map {
        cmd.arg("--map").arg(m);
    }
    let status = cmd.status().map_err(|e| format!("cannot start razdor: {e}"))?;
    Ok(if status.success() { 0 } else { 1 })
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
