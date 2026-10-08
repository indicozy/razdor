//! Sounds and music of the install: the `_Sounds.ini` table and the `Sounds/` files.
//!
//! - `_Sounds.ini` (cp1251) has two sections: `[Backgrounds]` (music: `BkgMap1..7`,
//!   `BkgMenuMain`, `BkgBattle1..2`, `BkgTriumph`, `BkgDefeat`, `BkgAuthors`) and
//!   `[SFX-Effects]` (interface, battle and item sounds). Each key names a file in `Sounds/`;
//!   names are matched ignoring case (`-Gold.wav` is `-Gold.WAV` on disk).
//! - `*.wav`: RIFF PCM, 8-bit mono 22050 Hz (one at 11025 Hz).
//! - `*.raw`: headerless signed 16-bit little-endian mono PCM. The sample rate is not stored;
//!   Razdor assumes 22050 Hz *(guess: the ini says all PCM is 22050 Hz, and the spectrum rolls
//!   off just below 11 kHz as a 22050 Hz recording would)*. `RAZDOR_MUSIC_RATE` overrides it.
//!   A trailing odd byte is dropped.
//!
//! Everything is read from the player's install at runtime and converted in memory; nothing
//! is written anywhere.

use super::ini::Ini;
use super::install::{find_path, DtInstall};
use super::DtError;
use std::path::Path;

pub const SOUNDS_INI: &str = "_Sounds.ini";
pub const SOUNDS_DIR: &str = "Sounds";
pub const BACKGROUNDS: &str = "Backgrounds";
pub const EFFECTS: &str = "SFX-Effects";
/// Sample rate assumed for `.raw` files *(guess)*.
pub const DEFAULT_RAW_RATE: u32 = 22050;
/// Environment variable overriding [`DEFAULT_RAW_RATE`] (e.g. `44100`).
pub const RATE_ENV: &str = "RAZDOR_MUSIC_RATE";

/// The `_Sounds.ini` table: ini key → file name in `Sounds/`, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundTable {
    /// `[Backgrounds]`: music.
    pub backgrounds: Vec<(String, String)>,
    /// `[SFX-Effects]`: sound effects.
    pub effects: Vec<(String, String)>,
}

fn entries(ini: &Ini, section: &str) -> Vec<(String, String)> {
    ini.section(section)
        .map(|s| s.entries.iter().filter(|(_, v)| !v.is_empty()).cloned().collect())
        .unwrap_or_default()
}

fn lookup<'a>(list: &'a [(String, String)], key: &str) -> Option<&'a str> {
    list.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

impl SoundTable {
    pub fn from_ini(ini: &Ini) -> SoundTable {
        SoundTable { backgrounds: entries(ini, BACKGROUNDS), effects: entries(ini, EFFECTS) }
    }

    /// The music file of `key` (`BkgMap1`), ignoring case.
    pub fn background(&self, key: &str) -> Option<&str> {
        lookup(&self.backgrounds, key)
    }

    /// The effect file of `key` (`Battle-Fight`), ignoring case.
    pub fn effect(&self, key: &str) -> Option<&str> {
        lookup(&self.effects, key)
    }
}

/// Uncompressed PCM samples with their format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    /// 8 (unsigned) or 16 (signed little-endian).
    pub bits: u16,
    /// Interleaved samples, a whole number of frames.
    pub data: Vec<u8>,
}

impl Pcm {
    /// A headerless `.raw` file: signed 16-bit little-endian mono at `rate`. An odd trailing
    /// byte is dropped.
    pub fn from_raw(bytes: &[u8], rate: u32) -> Pcm {
        let even = bytes.len() & !1;
        Pcm { rate, channels: 1, bits: 16, data: bytes[..even].to_vec() }
    }

    fn block_align(&self) -> usize {
        self.channels as usize * (self.bits as usize / 8)
    }

    /// Sample frames (one sample per channel).
    pub fn frames(&self) -> usize {
        self.data.len() / self.block_align().max(1)
    }

    /// Playing time in seconds.
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }

    /// Sample `i` of the interleaved data as -1..1.
    fn sample(&self, i: usize) -> f32 {
        match self.bits {
            8 => (self.data[i] as f32 - 128.0) / 128.0,
            _ => i16::from_le_bytes([self.data[2 * i], self.data[2 * i + 1]]) as f32 / 32768.0,
        }
    }

    /// These samples at `rate`, 16-bit, by a windowed-sinc (Lanczos, 4 lobes) interpolator.
    /// The player's own resampler repeats the nearest sample, which adds a metallic buzz
    /// above the source's top frequency; the original's DirectSound filtered it out.
    pub fn resampled(&self, rate: u32) -> Pcm {
        const LOBES: i64 = 4;
        let (ch, frames) = (self.channels.max(1) as usize, self.frames());
        if self.rate == 0 || frames == 0 {
            return Pcm { rate, channels: self.channels, bits: 16, data: Vec::new() };
        }
        let (from, to) = (self.rate as u64, rate as u64);
        let out_frames = (frames as u64 * to / from) as usize;
        // Down-sampling narrows the filter to the new top frequency.
        let scale = (to as f64 / from as f64).min(1.0);
        let reach = (LOBES as f64 / scale).ceil() as i64;
        let lanczos = |x: f64| -> f64 {
            if x.abs() < 1e-9 {
                1.0
            } else if x.abs() >= LOBES as f64 {
                0.0
            } else {
                let px = std::f64::consts::PI * x;
                LOBES as f64 * px.sin() * (px / LOBES as f64).sin() / (px * px)
            }
        };
        // The filter has one phase per distinct fraction: o × from mod to, a multiple of
        // gcd(from, to) (two phases from 22050 Hz to 44100 Hz).
        let gcd = |mut a: u64, mut b: u64| {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        };
        let g = gcd(from, to);
        let phases: Vec<Vec<(i64, f32)>> = (0..to / g)
            .map(|ph| {
                let frac = (ph * g) as f64 / to as f64;
                let taps: Vec<(i64, f64)> = (1 - reach..=reach).map(|k| (k, lanczos((k as f64 - frac) * scale))).filter(|&(_, w)| w != 0.0).collect();
                let sum: f64 = taps.iter().map(|t| t.1).sum();
                taps.into_iter().map(|(k, w)| (k, (w / sum) as f32)).collect()
            })
            .collect();
        let src: Vec<f32> = (0..frames * ch).map(|i| self.sample(i)).collect();
        let last = frames as i64 - 1;
        let mut data = Vec::with_capacity(out_frames * ch * 2);
        for o in 0..out_frames {
            // Exact position: o × from / to source frames.
            let num = o as u64 * from;
            let base = (num / to) as i64;
            let taps = &phases[((num % to) / g) as usize];
            for c in 0..ch {
                let v: f32 = taps.iter().map(|&(k, w)| src[(base + k).clamp(0, last) as usize * ch + c] * w).sum();
                data.extend_from_slice(&((v * 32768.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes());
            }
        }
        Pcm { rate, channels: self.channels, bits: 16, data }
    }

    /// A canonical 44-byte-header RIFF WAVE file of these samples.
    pub fn to_wav(&self) -> Vec<u8> {
        let align = self.block_align() as u16;
        let data_len = (self.frames() * align as usize) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize + 1);
        out.extend_from_slice(b"RIFF");
        // RIFF size: everything after these 8 bytes, including the data pad byte.
        out.extend_from_slice(&(36 + data_len + (data_len & 1)).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&(self.rate * align as u32).to_le_bytes());
        out.extend_from_slice(&align.to_le_bytes());
        out.extend_from_slice(&self.bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(&self.data[..data_len as usize]);
        if data_len & 1 == 1 {
            out.push(0);
        }
        out
    }
}

/// A `.raw` file as a WAV file in memory (see [`Pcm::from_raw`]).
pub fn raw_to_wav(bytes: &[u8], rate: u32) -> Vec<u8> {
    Pcm::from_raw(bytes, rate).to_wav()
}

fn bad(what: &str) -> DtError {
    DtError::Sound(what.to_string())
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Reads a RIFF WAVE file: PCM, 8 or 16 bits, 1 or 2 channels. Unknown chunks are skipped;
/// a data chunk that claims more bytes than the file has is cut to what is there.
pub fn parse_wav(bytes: &[u8]) -> Result<Pcm, DtError> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(DtError::BadMagic { what: "WAV file" });
    }
    let mut fmt = None;
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let (id, size) = (&bytes[i..i + 4], u32_at(bytes, i + 4) as usize);
        let body = i + 8;
        let end = body.saturating_add(size).min(bytes.len());
        match id {
            b"fmt " if end - body >= 16 => {
                let (tag, channels, rate, bits) =
                    (u16_at(bytes, body), u16_at(bytes, body + 2), u32_at(bytes, body + 4), u16_at(bytes, body + 14));
                if tag != 1 {
                    return Err(bad("WAV is not plain PCM"));
                }
                if !(1..=2).contains(&channels) || !(bits == 8 || bits == 16) || rate == 0 {
                    return Err(bad("unsupported WAV format"));
                }
                fmt = Some((channels, rate, bits));
            }
            b"data" => {
                let (channels, rate, bits) = fmt.ok_or_else(|| bad("WAV data before its format"))?;
                let align = channels as usize * bits as usize / 8;
                let whole = body + (end - body) / align * align;
                return Ok(Pcm { rate, channels, bits, data: bytes[body..whole].to_vec() });
            }
            _ => {}
        }
        i = body.saturating_add(size).saturating_add(size & 1);
    }
    Err(bad("WAV has no data"))
}

/// Decodes the bytes of `file`: `.raw` at `raw_rate`, anything else as WAV.
pub fn decode(file: &str, bytes: &[u8], raw_rate: u32) -> Result<Pcm, DtError> {
    let is_raw = Path::new(file).extension().is_some_and(|e| e.eq_ignore_ascii_case("raw"));
    if is_raw {
        Ok(Pcm::from_raw(bytes, raw_rate))
    } else {
        parse_wav(bytes)
    }
}

/// The `.raw` sample rate: `RAZDOR_MUSIC_RATE` if it is a sensible number, else 22050.
pub fn raw_rate_from_env() -> u32 {
    parse_rate(std::env::var(RATE_ENV).ok().as_deref())
}

fn parse_rate(v: Option<&str>) -> u32 {
    v.and_then(|v| v.trim().parse().ok()).filter(|r| (4000..=192_000).contains(r)).unwrap_or(DEFAULT_RAW_RATE)
}

impl DtInstall {
    /// The install's `_Sounds.ini` (see [`read_table`]).
    pub fn sound_table(&self) -> Result<SoundTable, DtError> {
        read_table(&self.dir)
    }

    /// Reads and decodes `Sounds/<file>` (see [`read_sound`]).
    pub fn read_sound(&self, file: &str, raw_rate: u32) -> Result<Pcm, DtError> {
        read_sound(&self.dir, file, raw_rate)
    }
}

/// Reads `<install dir>/_Sounds.ini`.
pub fn read_table(dir: &Path) -> Result<SoundTable, DtError> {
    let path = find_path(dir, SOUNDS_INI)?;
    let bytes = std::fs::read(&path).map_err(|source| DtError::Io { path: path.clone(), source })?;
    Ok(SoundTable::from_ini(&Ini::from_cp1251(&bytes)))
}

/// Reads and decodes `<install dir>/Sounds/<file>`, the name matched ignoring case.
pub fn read_sound(dir: &Path, file: &str, raw_rate: u32) -> Result<Pcm, DtError> {
    let path = find_path(dir, &format!("{SOUNDS_DIR}/{file}"))?;
    let bytes = std::fs::read(&path).map_err(|source| DtError::Io { path: path.clone(), source })?;
    decode(file, &bytes, raw_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "// comment\r\n[Backgrounds]\r\nBkgMap1=map1.raw\r\nBkgMenuMain=menu.raw\r\nEmpty=\r\n\
        [SFX-Effects]\r\n// Interface\r\nInterfaceButtonDown=downbutton.wav\r\nItem-Gold=-Gold.wav\r\n";

    #[test]
    fn parses_the_table() {
        let t = SoundTable::from_ini(&Ini::parse(INI));
        assert_eq!(t.backgrounds.len(), 2);
        assert_eq!(t.background("bkgmap1"), Some("map1.raw"));
        assert_eq!(t.background("Empty"), None);
        assert_eq!(t.effect("InterfaceButtonDown"), Some("downbutton.wav"));
        assert_eq!(t.effect("Item-Gold"), Some("-Gold.wav"));
        assert_eq!(t.effect("BkgMap1"), None);
        assert_eq!(SoundTable::from_ini(&Ini::parse("")), SoundTable::default());
    }

    /// Checks every header field of a canonical WAV file.
    fn check_header(w: &[u8], channels: u16, rate: u32, bits: u16, data_len: u32) {
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32_at(w, 4) as usize, w.len() - 8);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32_at(w, 16), 16);
        assert_eq!(u16_at(w, 20), 1);
        assert_eq!(u16_at(w, 22), channels);
        assert_eq!(u32_at(w, 24), rate);
        let align = channels * bits / 8;
        assert_eq!(u32_at(w, 28), rate * align as u32);
        assert_eq!(u16_at(w, 32), align);
        assert_eq!(u16_at(w, 34), bits);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32_at(w, 40), data_len);
    }

    #[test]
    fn raw_becomes_a_wav() {
        let raw: Vec<u8> = (0..1000u32).flat_map(|i| (i as i16 * 7).to_le_bytes()).collect();
        let w = raw_to_wav(&raw, 22050);
        assert_eq!(w.len(), 44 + 2000);
        check_header(&w, 1, 22050, 16, 2000);
        assert_eq!(&w[44..], &raw[..]);
        let pcm = parse_wav(&w).unwrap();
        assert_eq!((pcm.rate, pcm.channels, pcm.bits, pcm.frames()), (22050, 1, 16, 1000));
        assert!((Pcm::from_raw(&raw, 44100).duration() - 1000.0 / 44100.0).abs() < 1e-12);
    }

    #[test]
    fn odd_raw_drops_the_last_byte() {
        let w = raw_to_wav(&[1, 2, 3, 4, 5], 22050);
        check_header(&w, 1, 22050, 16, 4);
        assert_eq!(&w[44..], &[1, 2, 3, 4]);
        assert_eq!(raw_to_wav(&[9], 22050).len(), 44);
    }

    #[test]
    fn eight_bit_wav_roundtrips_with_pad_byte() {
        let pcm = Pcm { rate: 11025, channels: 1, bits: 8, data: vec![128, 130, 126] };
        let w = pcm.to_wav();
        // Odd data: the RIFF size counts the pad byte, the data size does not.
        assert_eq!(w.len(), 44 + 4);
        check_header(&w, 1, 11025, 8, 3);
        assert_eq!(parse_wav(&w).unwrap(), pcm);
    }

    #[test]
    fn skips_unknown_chunks_and_cuts_short_data() {
        let mut w = Pcm { rate: 22050, channels: 2, bits: 16, data: vec![0; 8] }.to_wav();
        // A LIST chunk between fmt and data.
        let list = [b"LIST".as_slice(), &3u32.to_le_bytes(), &[1, 2, 3, 0]].concat();
        w.splice(36..36, list);
        // The data chunk claims 100 bytes; 8 are there.
        let d = w.len() - 8 - 4;
        w[d..d + 4].copy_from_slice(&100u32.to_le_bytes());
        let pcm = parse_wav(&w).unwrap();
        assert_eq!((pcm.channels, pcm.frames()), (2, 2));
    }

    #[test]
    fn rejects_bad_wavs() {
        assert!(matches!(parse_wav(b"RIFX0000WAVE"), Err(DtError::BadMagic { .. })));
        let mut w = Pcm { rate: 22050, channels: 1, bits: 8, data: vec![1] }.to_wav();
        w[20] = 3; // IEEE float
        assert!(parse_wav(&w).is_err());
        assert!(parse_wav(&w[..36]).is_err());
    }

    #[test]
    fn decode_by_extension_and_rate_override() {
        assert_eq!(decode("MAP1.RAW", &[0, 0, 1], 22050).unwrap().frames(), 1);
        assert!(decode("x.wav", &[0, 0, 1], 22050).is_err());
        assert_eq!(parse_rate(None), 22050);
        assert_eq!(parse_rate(Some(" 44100 ")), 44100);
        assert_eq!(parse_rate(Some("fast")), 22050);
        assert_eq!(parse_rate(Some("12")), 22050);
    }

    #[test]
    fn real_sound_files() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(Path::new(&dir)).unwrap();
        let t = dt.sound_table().unwrap();
        assert_eq!((t.backgrounds.len(), t.effects.len()), (13, 33));
        let mut music = 0.0;
        for (key, file) in t.backgrounds.iter().chain(&t.effects) {
            let pcm = dt.read_sound(file, DEFAULT_RAW_RATE).unwrap_or_else(|e| panic!("{key}={file}: {e}"));
            assert!(pcm.frames() > 0, "{key}");
            assert_eq!(parse_wav(&pcm.to_wav()).unwrap(), pcm, "{key}");
            if key.starts_with("Bkg") {
                music += pcm.duration();
            }
        }
        // About 10½ minutes of music at 22050 Hz.
        assert!((600.0..680.0).contains(&music), "{music}");
        assert_eq!(dt.read_sound("MUS_9.wav", 22050).unwrap().rate, 11025);
    }

    fn sine(rate: u32, hz: f64, frames: usize) -> Pcm {
        let data = (0..frames).flat_map(|i| (((i as f64 * hz / rate as f64 * std::f64::consts::TAU).sin() * 16000.0) as i16).to_le_bytes()).collect();
        Pcm { rate, channels: 1, bits: 16, data }
    }

    fn samples(p: &Pcm) -> Vec<f64> {
        (0..p.frames()).map(|i| p.sample(i) as f64).collect()
    }

    #[test]
    fn resampling_doubles_the_rate_without_a_buzz() {
        // An 8 kHz tone at 22050 Hz: repeating samples would leave its 14 kHz image at about
        // two thirds of its level; the filter keeps the tone and drops the image.
        let src = sine(22050, 8000.0, 22050);
        let up = src.resampled(44100);
        assert_eq!((up.rate, up.channels, up.bits, up.frames()), (44100, 1, 16, 44100));
        let s = samples(&up);
        // The source samples come back as they were.
        let orig = samples(&src);
        assert!((1000..2000).all(|i| (s[2 * i] - orig[i]).abs() < 1e-4));
        let level = |x: &[f64], hz: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, v) in x.iter().enumerate() {
                let a = i as f64 * hz / 44100.0 * std::f64::consts::TAU;
                re += v * a.cos();
                im += v * a.sin();
            }
            (re * re + im * im).sqrt() / x.len() as f64
        };
        let mid = &s[4000..40000];
        let (tone, image) = (level(mid, 8000.0), level(mid, 14050.0));
        assert!(image < tone / 30.0, "tone {tone}, image {image}");
    }

    #[test]
    fn resampling_keeps_channels_and_reads_8_bit() {
        let p = Pcm { rate: 11025, channels: 2, bits: 8, data: vec![128, 255, 128, 255, 128, 255, 128, 255] };
        let up = p.resampled(44100);
        assert_eq!((up.channels, up.bits, up.frames()), (2, 16, 16));
        let s = samples(&up);
        assert!(s.chunks(2).all(|f| f[0].abs() < 1e-4 && (f[1] - 127.0 / 128.0).abs() < 1e-3), "{s:?}");
        assert_eq!(Pcm { rate: 22050, channels: 1, bits: 16, data: vec![] }.resampled(44100).frames(), 0);
    }
}
