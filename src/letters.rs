//! Letters to Ded Moroz (the holiday look's «Письмо Деду Морозу»): the player's wish, written in the
//! window and kept on the disk until a server takes it. There is no server yet: a letter is appended
//! to `letters-pending.jsonl` in the settings folder, one JSON line each. Once [`LETTERS_URL`] is set,
//! the pending letters are POSTed on start, and a letter the server took (a 2xx answer) is removed.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The server that takes the letters. Empty: nothing is sent, the letters stay pending.
pub const LETTERS_URL: &str = "";
/// The most a letter may be, in characters (the window stops typing there).
pub const MAX_CHARS: usize = 1000;
/// The pending letters' file, in the settings folder.
pub const FILE: &str = "letters-pending.jsonl";

/// One letter as it is kept and sent: when it was written (Unix seconds), the interface language
/// (`ru` or `en`), the program's version and the text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Letter {
    pub time: u64,
    pub lang: String,
    pub version: String,
    pub text: String,
}

/// The text as it is kept: trimmed, and not empty and at most [`MAX_CHARS`] characters. None otherwise.
pub fn clean(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty() && t.chars().count() <= MAX_CHARS).then(|| t.to_string())
}

/// Where the pending letters are kept in the settings folder `dir`.
pub fn pending_path(dir: &Path) -> std::path::PathBuf {
    dir.join(FILE)
}

/// Appends a letter to the pending file (creating it and its folder). Refused when the text is
/// empty or too long.
pub fn append(dir: &Path, letter: &Letter) -> Result<(), String> {
    if clean(&letter.text).is_none() {
        return Err(format!("the letter is empty or longer than {MAX_CHARS} characters"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let line = serde_json::to_string(letter).map_err(|e| e.to_string())?;
    let path = pending_path(dir);
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "{line}").map_err(|e| format!("{}: {e}", path.display()))
}

/// The pending letters, in the order they were written. A line that is not a letter is skipped.
pub fn read_pending(dir: &Path) -> Vec<Letter> {
    let Ok(text) = std::fs::read_to_string(pending_path(dir)) else { return Vec::new() };
    text.lines().filter_map(|l| serde_json::from_str::<Letter>(l).ok()).collect()
}

/// Replaces the pending file with `keep` (the letters not sent yet); an empty list removes the file.
pub fn keep_only(dir: &Path, keep: &[Letter]) -> Result<(), String> {
    let path = pending_path(dir);
    if keep.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        };
    }
    let mut out = String::new();
    for l in keep {
        out.push_str(&serde_json::to_string(l).map_err(|e| e.to_string())?);
        out.push('\n');
    }
    crate::editor::files::write_atomically(&path, out.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Sends the pending letters to `url` with `post` (one JSON body each; it answers the HTTP status).
/// A 2xx answer removes the letter from the pending file, anything else keeps it. An empty `url`
/// sends nothing. Returns (sent, still pending).
pub fn send_pending(dir: &Path, url: &str, post: &mut dyn FnMut(&str, &str) -> Result<u16, String>) -> (usize, usize) {
    let pending = read_pending(dir);
    if url.is_empty() {
        return (0, pending.len());
    }
    let mut kept = Vec::new();
    for letter in &pending {
        let body = serde_json::to_string(letter).unwrap_or_default();
        match post(url, &body) {
            Ok(code) if (200..300).contains(&code) => {}
            Ok(code) => {
                crate::diag!("letters: the server answered {code}; the letter stays pending");
                kept.push(letter.clone());
            }
            Err(why) => {
                crate::diag!("letters: not sent ({why}); the letter stays pending");
                kept.push(letter.clone());
            }
        }
    }
    let sent = pending.len() - kept.len();
    if let Err(why) = keep_only(dir, &kept) {
        crate::diag!("letters: the pending file is not rewritten: {why}");
    }
    (sent, kept.len())
}

/// The HTTP POST of a letter. No HTTP client is built in yet, so it refuses: the send is only
/// reached once [`LETTERS_URL`] is set, and then this has to be filled in first.
pub fn post_json(_url: &str, _body: &str) -> Result<u16, String> {
    Err("no HTTP client in this build".into())
}

/// The start-up send: the pending letters go to [`LETTERS_URL`] (nothing while it is empty).
pub fn send_on_start(dir: &Path) -> (usize, usize) {
    send_pending(dir, LETTERS_URL, &mut post_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("razdor-letters-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn letter(text: &str) -> Letter {
        Letter { time: 1_800_000_000, lang: "ru".into(), version: "1.5.0".into(), text: text.into() }
    }

    #[test]
    fn a_letter_is_trimmed_and_kept_up_to_a_thousand_characters() {
        assert_eq!(clean("  Хочу коня  "), Some("Хочу коня".to_string()));
        assert_eq!(clean("   "), None, "an empty letter is no letter");
        assert!(clean(&"а".repeat(MAX_CHARS)).is_some(), "1000 characters is still a letter");
        assert_eq!(clean(&"а".repeat(MAX_CHARS + 1)), None, "1001 characters is not");
    }

    #[test]
    fn appended_letters_are_read_back_in_order() {
        let d = dir("append");
        append(&d, &letter("первое")).unwrap();
        append(&d, &letter("second")).unwrap();
        let got = read_pending(&d);
        assert_eq!(got.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["первое", "second"]);
        assert_eq!(got[0], letter("первое"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_or_too_long_letter_is_refused_and_nothing_is_written() {
        let d = dir("refuse");
        assert!(append(&d, &letter("  ")).is_err());
        assert!(append(&d, &letter(&"a".repeat(MAX_CHARS + 1))).is_err());
        assert!(read_pending(&d).is_empty());
        assert!(!pending_path(&d).exists());
    }

    #[test]
    fn a_bad_line_is_skipped_when_reading() {
        let d = dir("bad");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(pending_path(&d), format!("not json\n{}\n", serde_json::to_string(&letter("ok")).unwrap())).unwrap();
        assert_eq!(read_pending(&d), vec![letter("ok")]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn with_no_url_the_letters_stay_pending_and_nothing_is_posted() {
        let d = dir("nourl");
        append(&d, &letter("wait")).unwrap();
        let mut posts = 0;
        let (sent, kept) = send_pending(&d, "", &mut |_, _| {
            posts += 1;
            Ok(200)
        });
        assert_eq!((sent, kept), (0, 1));
        assert_eq!(posts, 0);
        assert_eq!(read_pending(&d).len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_2xx_answer_removes_the_letter_and_any_other_keeps_it() {
        let d = dir("send");
        append(&d, &letter("one")).unwrap();
        append(&d, &letter("two")).unwrap();
        let (sent, kept) = send_pending(&d, "https://example.org/letters", &mut |_, body| {
            Ok(if body.contains("one") { 201 } else { 500 })
        });
        assert_eq!((sent, kept), (1, 1));
        assert_eq!(read_pending(&d), vec![letter("two")]);
        let (sent, kept) = send_pending(&d, "https://example.org/letters", &mut |_, _| Ok(204));
        assert_eq!((sent, kept), (1, 0));
        assert!(!pending_path(&d).exists(), "nothing left: the file is removed");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_body_is_the_letter_as_json() {
        let d = dir("body");
        append(&d, &letter("хочу")).unwrap();
        let mut body = String::new();
        send_pending(&d, "https://example.org/x", &mut |_, b| {
            body = b.to_string();
            Ok(200)
        });
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["text"], "хочу");
        assert_eq!(v["lang"], "ru");
        assert_eq!(v["version"], "1.5.0");
        assert_eq!(v["time"], 1_800_000_000u64);
        let _ = std::fs::remove_dir_all(&d);
    }
}
