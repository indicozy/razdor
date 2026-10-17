//! `razdor-translate`: makes English copies of the install's Russian maps with an
//! OpenAI-compatible language model (`razdor::translate`). A window in Razdor's own style;
//! with `--map` it runs without one.
// Release builds for Windows open no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// Razdor's widgets and fonts; most of the game's screens go unused here.
#[allow(dead_code, unused_imports)]
#[path = "../../ui/mod.rs"]
mod ui;
mod view;

use macroquad::prelude::*;

const HELP: &str = "razdor-translate: English copies of Discord Times maps, by a language model

Without arguments: the window.
  --list                     the install's maps and campaigns
  --data                     translate the game data (units, items, spells, the game's
                             texts) into files for Razdor in the --language (Eng_*.ini
                             for English, e.g. Hindi-native_*.ini)
  --map <name>               translate this map (with its whole campaign), then write the
                             English copies into the game's maps folder, replacing older ones
                             (the name: a file name, its stem, or the start of it)
  --endpoint <url>           OpenAI-compatible API base (default http://localhost:11434/v1)
  --model <name>             model name
  --key <key>                API key (or the RAZDOR_LLM_KEY variable)
  --context <tokens>         the model's context window: sizes the batches
  --reasoning <effort>       reasoning_effort to send ('' to send none)
  --language <name>          the language to translate into (default English)
  --any-script               the language's own letters (umlauts, Devanagari...), Razdor
                             only: the original shows them garbled
  --notes <text>             what the model should know about the maps (context)
  --style <text>             your wishes for the translation's style (added to the rules)
  --fresh                    translate everything again (forget the saved translations)
  --show-prompt              print the instructions the model gets, then stop
  --save-settings            keep these settings for next time
  --no-write                 translate only; write the maps later";

/// The headless mode; `None` when the window should open.
fn cli(args: &[String]) -> Option<i32> {
    use razdor::translate::{self, llm::Settings};
    if args.is_empty() {
        return None;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return Some(0);
    }
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let Some(install) = razdor::dt::install::locate() else {
        eprintln!("No Discord Times install found (set RAZDOR_DT_DIR).");
        return Some(2);
    };
    let Some(maps_dir) = translate::maps_dir(&install) else {
        eprintln!("{} has no maps folder.", install.display());
        return Some(2);
    };
    let maps = translate::install_maps(&install);
    let pairs: Vec<(String, razdor::dt::dtm::Scenario)> = maps.iter().map(|(f, _, s)| (f.clone(), s.clone())).collect();
    if has("--list") {
        for (k, (file, s)) in pairs.iter().enumerate() {
            let chain = translate::job::campaign(&pairs, k);
            let tag = if chain.len() > 1 { format!("  [campaign, chapter {} of {}]", chain.iter().position(|&c| c == k).unwrap_or(0) + 1, chain.len()) } else { String::new() };
            println!("{file}: {}{tag}", s.title.trim());
        }
        return Some(0);
    }
    let data = has("--data");
    let name = value("--map");
    if name.is_none() && !data {
        eprintln!("{HELP}");
        return Some(2);
    }
    let found = name.as_ref().map(|name| {
        pairs
            .iter()
            .position(|(f, _)| f == name)
            .or_else(|| pairs.iter().position(|(f, _)| f.trim_end_matches(".DTm") == name))
            .or_else(|| pairs.iter().position(|(f, _)| f.to_lowercase().starts_with(&name.to_lowercase())))
    });
    if let (Some(name), Some(None)) = (&name, found) {
        eprintln!("No map {name:?} in {} (see --list).", maps_dir.display());
        return Some(2);
    }
    let mut s = Settings::load();
    if let Some(v) = value("--endpoint") {
        s.endpoint = v;
    }
    if let Some(v) = value("--model") {
        s.model = v;
    }
    if let Some(v) = value("--key").or_else(|| std::env::var("RAZDOR_LLM_KEY").ok()) {
        s.api_key = v;
    }
    if let Some(v) = value("--context").and_then(|v| v.parse().ok()) {
        s.context = v;
    }
    if let Some(v) = value("--reasoning") {
        s.reasoning_effort = v;
    }
    if let Some(v) = value("--language") {
        s.language = v;
    }
    if has("--any-script") {
        s.any_script = true;
    }
    if let Some(v) = value("--notes") {
        s.notes = v;
    }
    if let Some(v) = value("--style") {
        s.style = v;
    }
    if has("--save-settings") {
        if let Err(e) = s.save() {
            eprintln!("settings: {e}");
        }
    }
    let (mut job, path, target, what) = if data {
        let data = translate::data::install_texts(&install);
        let (job, path) = translate::data::open_job(&data, &s.language, s.any_script);
        (job, path, translate::Target::Data { install: install.clone(), data }, "the game data".to_string())
    } else {
        let at = found.flatten().expect("a map was found");
        let chain: Vec<(String, razdor::dt::dtm::Scenario)> = translate::job::campaign(&pairs, at).into_iter().map(|k| pairs[k].clone()).collect();
        let (job, path) = translate::open_job(&chain, &s.language, s.any_script);
        let what = format!("{} map(s): {}", chain.len(), chain.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", "));
        (job, path, translate::Target::Maps(maps_dir.clone()), what)
    };
    println!("{what}; {} strings, {} done; model {} at {}, context {}", job.entries.len(), job.done(), s.model, s.endpoint, s.context);
    if let Some(p) = &path {
        println!("progress file: {}", p.display());
        println!("request log: {}", translate::request_log(p).display());
    }
    if has("--show-prompt") {
        println!("{}", translate::llm::system_prompt(&translate::llm::Settings { language: job.language.clone(), any_script: job.any_script, ..s.clone() }));
        return Some(0);
    }
    if has("--fresh") {
        job.forget_all();
    }
    job.retry_failed();
    let stop = std::sync::atomic::AtomicBool::new(false);
    let total = job.entries.len();
    let result = translate::run(&mut job, path.as_deref(), &s, &stop, |job, e| match e {
        translate::Event::Sending { items, first } => println!("[{}/{total}] sending {items}: {first}…", job.done()),
        translate::Event::Answer { taken, of, seconds } => println!("    took {taken}/{of} in {seconds:.1} s"),
        translate::Event::Failed { why, request } => println!("    failed: {why} (request {request} in the request log)"),
        translate::Event::Refused { request, texts } => {
            for r in texts {
                let answer = r.answer.as_deref().map_or("-".to_string(), |a| translate::text::excerpt(a, 80));
                println!("    refused [{}] «{}»: {}\n        translation «{answer}»; request {request} in the request log", r.key, translate::text::excerpt(&r.src, 60), r.why);
            }
        }
    });
    if let Err(why) = result {
        eprintln!("Stopped: {why}");
        return Some(1);
    }
    for e in job.entries.iter().filter(|e| e.en.is_none()) {
        println!("given up: {:?}: {}", e.src.chars().take(80).collect::<String>(), e.last_error);
    }
    println!("{} of {total} strings translated.", job.done());
    if has("--no-write") {
        return Some(0);
    }
    match target.write(&job, true) {
        Ok(files) => {
            for f in files {
                println!("wrote {}", f.display());
            }
            Some(0)
        }
        Err(why) => {
            eprintln!("Not written: {why}");
            Some(1)
        }
    }
}

fn conf() -> Conf {
    razdor::dt::install::load_dotenv();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli(&args) {
        std::process::exit(code);
    }
    let (window_width, window_height) = ui::snapshot::size().unwrap_or((1180, 840));
    Conf {
        window_title: "Razdor: map translator".to_owned(),
        window_width,
        window_height,
        high_dpi: true,
        icon: Some(macroquad::miniquad::conf::Icon {
            small: *include_bytes!("../../../data/icon/window-16.rgba"),
            medium: *include_bytes!("../../../data/icon/window-32.rgba"),
            big: *include_bytes!("../../../data/icon/window-64.rgba"),
        }),
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    // Closing the window sets a flag instead of leaving the loop, so the exit skips the
    // native libraries' exit handlers (see `exit_now` in src/main.rs).
    prevent_quit();
    ui::widgets::load_font().await;
    ui::language::init();
    let mut view = view::View::new();
    // Off-screen checks: `RAZDOR_SCENE=translate:<map>` picks a map, `RAZDOR_SNAPSHOT` saves
    // the window after `RAZDOR_SNAPSHOT_FRAMES` frames and quits (the `offscreen-snapshot` skill).
    if let Some(name) = std::env::var("RAZDOR_SCENE").ok().and_then(|s| s.strip_prefix("translate:").map(str::to_string)) {
        view.pick(&name);
    }
    let snapshot = ui::snapshot::target();
    let mut frames = 0u64;
    loop {
        view.frame();
        frames += 1;
        let shot = snapshot.as_ref().is_some_and(|(_, n)| frames >= *n);
        if let Some((path, _)) = snapshot.as_ref().filter(|_| shot) {
            get_screen_data().export_png(path);
        }
        if is_quit_requested() || shot {
            view.shutdown();
            // SAFETY: `_exit` only ends the process; no Rust state is used after it.
            #[cfg(unix)]
            unsafe {
                libc::_exit(0)
            }
            #[cfg(not(unix))]
            std::process::exit(0)
        }
        next_frame().await;
    }
}
