use razdor::dt::sound;
fn main() {
    let dir = std::path::PathBuf::from(std::env::var("RAZDOR_DT_DIR").unwrap());
    let table = sound::read_table(&dir).unwrap();
    for t in ["BkgMap1", "BkgMap2", "BkgMap3", "BkgMenu", "BkgBattle1"] {
        let Some(file) = table.background(t) else { continue };
        let t0 = std::time::Instant::now();
        let pcm = sound::read_sound(&dir, file, sound::raw_rate_from_env()).unwrap();
        let t1 = t0.elapsed();
        let r = pcm.resampled(44100);
        let t2 = t0.elapsed();
        let wav = r.to_wav();
        let t3 = t0.elapsed();
        println!("{t} {file}: {:.1} s audio, decode {:?}, resample {:?}, wav {:?} ({} MB)", pcm.duration(), t1, t2 - t1, t3 - t2, wav.len() / 1_000_000);
    }
}
