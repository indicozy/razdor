//! `razdor-server`: serves the API (`razdor-server [--config FILE]`, or `RAZDOR_CONFIG`), or runs one
//! of the helpers: `hash-password` (the admin password from stdin, as an argon2 hash) and
//! `token-hash` (the admin token from stdin, as the SHA-256 for the config).

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;

use razdor_server::clock::Clock;
use razdor_server::config::Config;
use razdor_server::state::{sha256_hex, AppState};

fn read_line_from_stdin() -> Result<String, String> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    let line = line.trim_end_matches(['\n', '\r']).to_string();
    if line.is_empty() {
        return Err("nothing on stdin".into());
    }
    Ok(line)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hash-password") => match read_line_from_stdin().and_then(|p| razdor_server::admin::hash_password(&p)) {
            Ok(hash) => {
                println!("{hash}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("hash-password: {e}");
                ExitCode::FAILURE
            }
        },
        Some("token-hash") => match read_line_from_stdin() {
            Ok(token) => {
                println!("{}", sha256_hex(&[token.as_bytes()]));
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("token-hash: {e}");
                ExitCode::FAILURE
            }
        },
        _ => serve(&args),
    }
}

fn serve(args: &[String]) -> ExitCode {
    let mut file = std::env::var_os("RAZDOR_CONFIG").map(PathBuf::from);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" => match it.next() {
                Some(path) => file = Some(PathBuf::from(path)),
                None => {
                    eprintln!("--config needs a file");
                    return ExitCode::FAILURE;
                }
            },
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    let cfg = match Config::load(file.as_deref(), |k| std::env::var(k).ok()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config: {e}");
            return ExitCode::FAILURE;
        }
    };
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    let blocking_threads = cfg.blocking_threads;
    let state = match AppState::open(cfg, Clock::system()) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("opening the data: {e}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(blocking_threads)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(razdor_server::serve(state)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("serve: {e}");
            ExitCode::FAILURE
        }
    }
}
