//! `razdor-openmw <map.json> -o <out dir> [--name <id>] [--tile-units 1024] [--template <path>]`
//!
//! Writes `<out>/<name>.omwaddon` and `<out>/textures/razdor/*.tga`. Exit codes: 0 done, 1 a file
//! could not be written, 2 bad input or usage.

use razdor_openmw::convert::{self, Master, Options};
use razdor_openmw::input::Map;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: razdor-openmw <map.json> -o <out dir> [--name <id>] [--tile-units 512|1024|2048] [--template <path to template.omwgame>] [--debug-heightmap <pgm path>]";

#[derive(Debug)]
enum CliError {
    Usage(String),
    Io(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(m) | CliError::Io(m) => write!(f, "{m}"),
        }
    }
}

struct Args {
    input: PathBuf,
    out: PathBuf,
    name: String,
    tile_units: i64,
    template: Option<PathBuf>,
    debug_heightmap: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Option<Args>, CliError> {
    let mut input = None;
    let mut out = None;
    let mut name = "razdor_map".to_string();
    let mut tile_units = 1024i64;
    let mut template = None;
    let mut debug_heightmap = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| -> Result<String, CliError> {
            it.next().cloned().ok_or_else(|| CliError::Usage(format!("{flag} needs a value\n{USAGE}")))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "-o" | "--out" => out = Some(PathBuf::from(value("-o")?)),
            "--name" => name = value("--name")?,
            "--tile-units" => {
                let v = value("--tile-units")?;
                tile_units = v.parse().map_err(|_| CliError::Usage(format!("--tile-units {v} is not a number")))?;
            }
            "--template" => template = Some(PathBuf::from(value("--template")?)),
            "--debug-heightmap" => debug_heightmap = Some(PathBuf::from(value("--debug-heightmap")?)),
            flag if flag.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option {flag}\n{USAGE}")));
            }
            path => {
                if input.is_some() {
                    return Err(CliError::Usage(format!("only one map file may be given\n{USAGE}")));
                }
                input = Some(PathBuf::from(path));
            }
        }
    }
    let input = input.ok_or_else(|| CliError::Usage(format!("no map file given\n{USAGE}")))?;
    let out = out.ok_or_else(|| CliError::Usage(format!("no output folder (-o) given\n{USAGE}")))?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(CliError::Usage(format!("--name {name} may hold only letters, digits, '_' and '-'")));
    }
    if ![512, 1024, 2048].contains(&tile_units) {
        return Err(CliError::Usage(format!("--tile-units must be 512, 1024 or 2048, not {tile_units}")));
    }
    Ok(Some(Args { input, out, name, tile_units, template, debug_heightmap }))
}

/// The master's name and size: the file name of `--template` and its size, or `template.omwgame`
/// with size 0 when no path is given.
fn master_of(template: &Option<PathBuf>) -> Result<Master, CliError> {
    match template {
        None => Ok(Master { name: "template.omwgame".to_string(), size: 0 }),
        Some(path) => {
            let size = std::fs::metadata(path)
                .map_err(|e| CliError::Usage(format!("cannot read template {}: {e}", path.display())))?
                .len();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| CliError::Usage(format!("bad template path {}", path.display())))?;
            Ok(Master { name: name.to_string(), size })
        }
    }
}

fn run(args: &Args) -> Result<(), CliError> {
    let text = std::fs::read_to_string(&args.input)
        .map_err(|e| CliError::Usage(format!("cannot read {}: {e}", args.input.display())))?;
    let map = Map::from_json(&text).map_err(|e| CliError::Usage(e.to_string()))?;
    let options = Options { name: args.name.clone(), tile_units: args.tile_units, master: master_of(&args.template)? };
    let out = convert::convert(&map, &options).map_err(|e| CliError::Usage(e.to_string()))?;

    let tex_dir = args.out.join("textures").join("razdor");
    std::fs::create_dir_all(&tex_dir).map_err(|e| io_error(&tex_dir, &e))?;
    let plugin_path = args.out.join(format!("{}.omwaddon", args.name));
    write(&plugin_path, &out.plugin)?;
    for (rel, bytes) in &out.textures {
        write(&args.out.join(rel), bytes)?;
    }
    let mesh_dir = args.out.join("meshes").join("razdor");
    std::fs::create_dir_all(&mesh_dir).map_err(|e| io_error(&mesh_dir, &e))?;
    for (rel, bytes) in &out.meshes {
        write(&args.out.join(rel), bytes)?;
    }
    if let Some(path) = &args.debug_heightmap {
        write(path, &out.heights.pgm())?;
    }
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    std::fs::write(path, bytes).map_err(|e| io_error(path, &e))
}

fn io_error(path: &Path, e: &std::io::Error) -> CliError {
    CliError::Io(format!("cannot write {}: {e}", path.display()))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("razdor-openmw: {e}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e @ CliError::Usage(_)) => {
            eprintln!("razdor-openmw: {e}");
            ExitCode::from(2)
        }
        Err(e @ CliError::Io(_)) => {
            eprintln!("razdor-openmw: {e}");
            ExitCode::from(1)
        }
    }
}
