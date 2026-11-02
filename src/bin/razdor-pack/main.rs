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
                             [--trace] [--choices first|random|<dialog>=<choice>,…]
                             [--set var=value]… [--seed <n>] [--campaign [--auto-win-after <days>]]
                             --set forces a story variable before the story starts (a
                             developer switch to drive branches; repeatable; on a campaign it
                             is set for its first map and carries on).
                             --campaign plays the whole campaign (campaign.toml) from its
                             start: each map for n days, and a victory or a travel goes to the
                             next map with what carries; --auto-win-after fires each map's
                             victory after that many days (a developer switch: the hero does
                             not walk in the sim, so this walks the graph).
                             the AI plays a map headless for n days, the community map
                             smoke test's way (RAZDOR_DT_DIR): a panic, a softlock or a
                             check error fails; reports the day of victory or defeat.
                             Without --map every map of the package is played.
                             The package's story runs with the map: --trace prints each
                             day's fired triggers, variable changes, dialogs and the
                             choice taken (--json gives it as data); --choices says how
                             dialogs are answered (first by default; random is seeded;
                             dialog=choice forces that choice, the first elsewhere).
                             exit status: 0 all played, 1 a panic, a softlock or errors
  razdor-pack play <folder-or-.rzpack> [--map <id>]
                             opens the package's map in Razdor (`razdor --pack`)
  razdor-pack pack <folder> [-o <out.rzpack>]
                             checks the folder (as check does; an error refuses it), then
                             zips it deterministically: the same folder gives the same
                             bytes and SHA-256. Writes <folder-name>.rzpack in the current
                             folder unless -o names the file; prints its size and SHA-256
  razdor-pack info <folder-or-.rzpack>
                             the manifest, the maps, the size and SHA-256 of a package
  razdor-pack gen-aliases [--out <path>]
                             write the dt: alias table of the install (RAZDOR_DT_DIR, or
                             the one Razdor finds) with Razdor's English names
                             (default data/pack/dt-aliases.toml)
  --help                     this text

Not built yet: render (pictures of a package).";

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
        Some("pack") => pack_cmd(&args[1..]),
        Some("info") => info_cmd(&args[1..]),
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

/// The arguments of `sim` and `play`: a package, `--map <id>`, `--days <n>`, `--json`, `--trace`,
/// `--choices <spec>`.
#[derive(Default)]
struct Args {
    path: Option<PathBuf>,
    map: Option<String>,
    days: Option<u64>,
    json: bool,
    trace: bool,
    choices: Option<String>,
    campaign: bool,
    auto_win: Option<u64>,
    set: Vec<String>,
    seed: Option<u32>,
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
            "--trace" if allow_sim => out.trace = true,
            "--campaign" if allow_sim => out.campaign = true,
            "--seed" if allow_sim => {
                let v = it.next().ok_or("--seed needs a number")?;
                out.seed = Some(v.parse().map_err(|_| format!("--seed {v}: a whole number"))?);
            }
            "--auto-win-after" if allow_sim => {
                let v = it.next().ok_or("--auto-win-after needs a number of days")?;
                out.auto_win = Some(v.parse().map_err(|_| format!("--auto-win-after {v}: a whole number of days"))?);
            }
            "--choices" if allow_sim => out.choices = Some(it.next().ok_or("--choices needs first, random or dialog=choice,…")?.clone()),
            "--set" if allow_sim => out.set.push(it.next().ok_or("--set needs var=value")?.clone()),
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
    if a.campaign {
        return sim_campaign_cmd(&package, &a);
    }
    if a.auto_win.is_some() {
        return Err("--auto-win-after needs --campaign".into());
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
    let preset = parse_presets(&package, &a)?;
    let opts = pack::sim::SimOptions {
        days: a.days.unwrap_or(30),
        choices: match &a.choices {
            Some(spec) => pack::sim::Choices::parse(spec)?,
            None => pack::sim::Choices::First,
        },
        // The JSON carries the trace too: `--json` is the same data as `--trace`.
        trace: a.trace || a.json,
        preset: preset.clone(),
        seed: a.seed,
    };
    let mut failed = false;
    let mut json = Vec::new();
    for map in &maps {
        match pack::sim::sim_map(&package, &install, map, &opts) {
            Ok(result) => {
                failed |= !result.ok();
                if a.json {
                    json.push(result.json());
                } else {
                    println!("{}", result.text());
                    if a.trace {
                        print!("{}", result.trace_text());
                    }
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

/// The `--set` variables of a sim, read and checked against the package's declarations.
fn parse_presets(package: &Package, a: &Args) -> Result<Vec<(String, pack::triggers::VarValue)>, String> {
    let preset = a.set.iter().map(|s| pack::sim::parse_set(s)).collect::<Result<Vec<_>, _>>()?;
    pack::sim::check_presets(package, &preset)?;
    Ok(preset)
}

/// `sim --campaign`: the campaign from its start, each map for `--days`, the graph walked on the
/// victories and travels. Exit status 1 on a panic, a softlock or an error.
fn sim_campaign_cmd(package: &Package, a: &Args) -> Result<u8, String> {
    if a.map.is_some() {
        return Err("--campaign plays from the campaign's start: --map does not go with it".into());
    }
    if package.campaign.is_none() {
        return Err("the package has no campaign.toml (see docs/reference/rzpack-format.md, Campaigns)".into());
    }
    razdor::dt::install::load_dotenv();
    let dir = razdor::dt::install::locate().ok_or("no Discord Times install found (set RAZDOR_DT_DIR)")?;
    let install = razdor::dt::install::DtInstall::load(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let preset = parse_presets(package, a)?;
    let opts = pack::sim::CampaignOptions {
        days: a.days.unwrap_or(60),
        choices: match &a.choices {
            Some(spec) => pack::sim::Choices::parse(spec)?,
            None => pack::sim::Choices::First,
        },
        trace: a.trace || a.json,
        auto_win_after: a.auto_win,
        preset,
        save_on_revisit: false,
    };
    let run = pack::sim::sim_campaign(package, &install, &opts).map_err(|r| r.to_text())?;
    if a.json {
        println!("{}", run.json());
    } else {
        print!("{}", run.text());
    }
    Ok(if run.ok() { 0 } else { 1 })
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

/// `pack <folder> [-o out]`: the package checked and zipped (`pack::archive`).
fn pack_cmd(args: &[String]) -> Result<u8, String> {
    let mut out: Option<PathBuf> = None;
    let mut folder: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--out" => out = Some(PathBuf::from(it.next().ok_or("-o needs a file name")?)),
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}` for pack")),
            p if folder.is_none() => folder = Some(PathBuf::from(p)),
            _ => return Err("pack takes one folder".into()),
        }
    }
    let folder = folder.ok_or("pack needs a package folder")?;
    let out = match out {
        Some(o) => o,
        None => {
            let name = folder.file_name().map(|n| n.to_string_lossy().trim_end_matches(".rzpack").to_string()).unwrap_or_else(|| "package".into());
            PathBuf::from(format!("{name}.rzpack"))
        }
    };
    let packed = match pack::archive::pack(&folder) {
        Ok(p) => p,
        Err(text) => {
            eprint!("{text}");
            eprintln!("razdor-pack: {} is not packed: fix the errors above", folder.display());
            return Ok(1);
        }
    };
    std::fs::write(&out, &packed.bytes).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!("wrote {}: {} bytes", out.display(), packed.bytes.len());
    println!("sha256 {}", packed.sha256);
    Ok(0)
}

/// `info <folder-or-.rzpack>`: the manifest, the maps and the size of a package.
fn info_cmd(args: &[String]) -> Result<u8, String> {
    let [path] = args else { return Err("info takes one folder or .rzpack file".into()) };
    let path = PathBuf::from(path);
    let files = Files::load(&path)?;
    let size = match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => m.len(),
        _ => files.iter().map(|(_, b)| b.len() as u64).sum(),
    };
    let (package, _) = Package::parse(files);
    let Some(m) = package.manifest.as_ref() else {
        print!("{}", pack::check(&package).to_text());
        return Ok(1);
    };
    let ru = |key: &str| package.texts.get("ru").and_then(|t| t.get(key)).cloned().unwrap_or_else(|| key.to_string());
    println!("package {} {} (format {})", m.id, m.version, m.format);
    println!("title: {}", ru(&m.title));
    if let Some(d) = &m.description {
        println!("description: {}", ru(d));
    }
    println!("author: {}", m.author);
    println!("license: {}", m.license);
    println!("base: {}", m.base);
    println!("rules: {}", m.rules);
    println!("requires: {}", if m.requires.is_empty() { "none".into() } else { m.requires.join(", ") });
    if package.maps.is_empty() {
        println!("maps: none");
    } else {
        println!("maps:");
        for id in package.maps.keys() {
            println!("  {id}");
        }
    }
    println!("campaign: none (format 1 has none)");
    println!("size: {size} bytes");
    if path.is_file() {
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("sha256: {}", razdor::update::sha256_hex(&bytes));
    }
    Ok(0)
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
