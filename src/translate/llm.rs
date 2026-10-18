//! The model: any OpenAI-compatible chat API (OpenAI, OpenRouter, DeepSeek, a local Ollama or
//! LM Studio), reached through the system's `curl` as the updater does (`crate::update`).
//! The key is handed to curl in a config file only this user can read, never on its command
//! line.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::text::prepare;
use crate::i18n::tr;
use crate::trf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The API's base, up to and with `/v1`.
    pub endpoint: String,
    /// Empty for a local server.
    pub api_key: String,
    pub model: String,
    /// The model's context window in tokens: it sizes the batches.
    pub context: u32,
    /// Sent as `reasoning_effort` when not empty (`none` turns thinking off in Ollama); left
    /// out by itself if the server refuses it.
    pub reasoning_effort: String,
    /// Ask for a JSON object (`response_format`).
    pub json_mode: bool,
    pub temperature: f32,
    /// Seconds one answer may take.
    pub timeout: u32,
    /// The player's own instructions on the translation's style (an author's voice, a
    /// glossary, a tone), added to the fixed rules ([`system_prompt`]).
    pub style: String,
    /// The language translated into, as the player writes it ("English", "German", "Hindi").
    pub language: String,
    /// The language's own letters (Devanagari, Chinese characters, umlauts), stored as UTF-8:
    /// Razdor only, the original cannot show them. Off: Latin without accents or Cyrillic,
    /// which the original's maps and fonts hold.
    pub any_script: bool,
    /// What the player tells the model about the maps (who is who, the lore, names to keep),
    /// sent with every batch.
    pub notes: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            endpoint: "http://localhost:11434/v1".into(),
            api_key: String::new(),
            model: "gemma4".into(),
            context: 8192,
            reasoning_effort: "none".into(),
            json_mode: true,
            temperature: 0.2,
            timeout: 600,
            style: String::new(),
            language: "English".into(),
            any_script: false,
            notes: String::new(),
        }
    }
}

impl Settings {
    /// `translate.json` next to Razdor's remembered install (`crate::dt::install::config_file`).
    pub fn path() -> Option<PathBuf> {
        Some(crate::dt::install::config_file()?.with_file_name("translate.json"))
    }

    pub fn load() -> Settings {
        Settings::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Settings::path().ok_or_else(|| std::io::Error::other("no config folder"))?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        // It may hold the key: written owner-only from the start, then renamed into place.
        let tmp = path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);
        write_private(&tmp, json.as_bytes())?;
        std::fs::rename(&tmp, &path)
    }

    /// Characters of Russian per batch: about 2.5 characters a token, the answer as long
    /// again, and room for the instructions and the glossary.
    pub fn batch_chars(&self) -> usize {
        (self.context as usize * 2 / 5).clamp(400, 6000)
    }

    /// Entries per batch: small models lose count in long lists.
    pub fn batch_items(&self) -> usize {
        if self.context >= 32768 { 60 } else { 30 }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", api_base(&self.endpoint), path)
    }
}

/// Creates `path` readable by its owner only from the start, never reusing a file (or a
/// symlink planted) there: the shared temp folder is open to every user.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)?.write_all(bytes)
}

/// The API's base from the address typed: a bare server address (`http://127.0.0.1:1234`, as
/// LM Studio shows it) gets the OpenAI path `/v1`; an address with a path is kept as it is.
pub fn api_base(endpoint: &str) -> String {
    let e = endpoint.trim().trim_end_matches('/');
    let after_host = e.split_once("://").map_or(e, |(_, rest)| rest);
    if after_host.contains('/') || e.is_empty() { e.to_string() } else { format!("{e}/v1") }
}

/// The instructions sent with every batch: the fixed rules, then the player's style, if any.
pub fn system_prompt(s: &Settings) -> String {
    let style = s.style.trim();
    let mut out = rules_styled(&s.language, s.any_script, !style.is_empty());
    let notes = s.notes.trim();
    if !notes.is_empty() {
        out.push_str(&format!("\n\nWhat the player tells you about these maps (use it to understand who is who and to choose words):\n{notes}"));
    }
    if !style.is_empty() {
        out.push_str(&format!(
            "\n\nTHE STYLE OF THIS TRANSLATION, chosen by the player: it decides the tone, the voice and the wording of every text. \
Only the technical rules above (the keys, placeholders, lines, markup marks and letters) come before it:\n{style}"
        ));
    }
    out
}

/// The fixed rules sent with every batch, for translating into `language`.
pub fn rules(language: &str, any_script: bool) -> String {
    rules_styled(language, any_script, false)
}

/// [`rules`]; with the player's own style (`styled`) the default tone line gives way to it.
fn rules_styled(language: &str, any_script: bool, styled: bool) -> String {
    let lang = match language.trim() {
        "" => "English",
        l => l,
    };
    if is_source_language(lang) {
        return rewrite_rules(styled);
    }
    let english = super::job::lang_suffix(lang) == "en";
    let letters = if any_script && !english {
        format!("Write {lang} in its own script and letters (with its accents, or Devanagari, Chinese characters and so on), as a native reader expects.")
    } else if english {
        "Use plain ASCII only: straight quotes, '-' for dashes, '...'; no Cyrillic in the answer.".to_string()
    } else {
        let cyrillic = if super::text::is_cyrillic_language(lang) { "" } else { " No Cyrillic in the answer." };
        format!(
            "The game stores text in the Windows-1251 code page: write only Latin letters without accents, \
Cyrillic letters, digits and plain punctuation (straight quotes, '-', '...'); write accented letters without \
their accents, and a language with another script in Latin letters (a common romanisation).{cyrillic}"
        )
    };
    let tone = if styled {
        "Style: the player's style given below decides the tone and wording; keep each text about as long as the Russian unless the style says otherwise."
    } else {
        "Style: medieval fantasy, plain and vivid, the tone of the original. Keep it about as long as the Russian."
    };
    format!(
        "You translate texts of the Russian fantasy strategy game \"Discord Times\" (\"Времена раздора\") into \
{lang} for its {lang} edition.

You get a JSON object of numbered Russian texts. Reply with ONLY a JSON object with the same keys, \
each value the {lang} translation of that text. Translate every entry; never merge, split, skip or \
add entries; never explain.

Keep exactly, in the same places:
- placeholders like {{0}}, {{1}};
- <br> and line breaks (\\n): the same number of lines;
- the markup characters * | @ ^ (they style a line; keep each where it stands, usually at the line's start).

{tone} \
Names: translate names that have a meaning (\"Проклятое озеро\" -> \"Cursed Lake\" in English); \
transliterate personal names (\"Иван\" -> \"Ivan\"). Use the glossary's {lang} for the names it lists. \
{letters}"
    )
}

/// The target is the maps' own language: the task is to rewrite, not to translate (translating
/// Russian into Russian gives back the text as it is, whatever the style says).
pub fn is_source_language(language: &str) -> bool {
    let l = language.trim().to_lowercase();
    l == "ru" || l.starts_with("russian") || l.starts_with("русск") || l == "рус"
}

/// The rules for rewriting the Russian texts in Russian in the player's style.
fn rewrite_rules(styled: bool) -> String {
    let task = if styled {
        "Rewrite each text in Russian in the player's style given below: change the wording, the tone and the voice as that style asks, keeping what happens and who is who. Never return a text unchanged unless it is a short name that the style would not change."
    } else {
        "Rewrite each text in good modern Russian, keeping its meaning, its names and the medieval fantasy tone."
    };
    format!(
        "You rewrite the texts of the Russian fantasy strategy game \"Discord Times\" (\"Времена раздора\") in Russian for a new edition.

You get a JSON object of numbered Russian texts. Reply with ONLY a JSON object with the same keys, \
each value the rewritten Russian text. Rewrite every entry; never merge, split, skip or add entries; never explain.

Keep exactly, in the same places:
- placeholders like {{0}}, {{1}};
- <br> and line breaks (\\n): the same number of lines;
- the markup characters * | @ ^ (they style a line; keep each where it stands, usually at the line's start).

{task} Personal names stay recognisable. Write Russian letters, digits and plain punctuation (straight \
quotes, '-', '...')."
    )
}

/// The user message for `texts` (the batch's Russian, in order, numbered from 1), with the
/// glossary in `language` and the player's style repeated at its end (models follow what
/// stands in the message itself more closely than the instructions).
pub fn user_prompt(texts: &[&str], glossary: &[(String, String)], language: &str, style: &str) -> String {
    let lang = match language.trim() {
        "" => "English",
        l => l,
    };
    let mut out = String::new();
    if !glossary.is_empty() {
        out.push_str(&format!("Glossary (Russian = {lang}):\n"));
        for (ru, en) in glossary {
            out.push_str(&format!("{ru} = {en}\n"));
        }
        out.push('\n');
    }
    let obj: serde_json::Map<String, Value> =
        texts.iter().enumerate().map(|(k, t)| ((k + 1).to_string(), Value::String(prepare(t).text))).collect();
    if is_source_language(lang) {
        out.push_str("Rewrite in Russian:\n");
    } else {
        out.push_str(&format!("Translate into {lang}:\n"));
    }
    out.push_str(&serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_default());
    if !style.trim().is_empty() {
        out.push_str(&format!("\n\nWrite every text in this style: {}", style.trim()));
    }
    out
}

/// The JSON object in a model's answer (code fences and chatter around it ignored): its
/// string values by key.
pub fn parse_answer(answer: &str) -> Result<HashMap<String, String>, String> {
    if answer.trim().is_empty() {
        return Err(tr("the answer is empty").to_string());
    }
    let no_json = || tr("no JSON object in the answer").to_string();
    let start = answer.find('{').ok_or_else(no_json)?;
    let end = answer.rfind('}').ok_or_else(no_json)?;
    if end < start {
        return Err(no_json());
    }
    let body = &answer[start..=end];
    match serde_json::from_str::<Value>(body) {
        Ok(v) => {
            let obj = v.as_object().ok_or_else(|| tr("the answer is not a JSON object").to_string())?;
            Ok(obj.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        }
        // A model sometimes breaks one entry (an unquoted value, a stray character): the
        // well-formed `"N": "text"` pairs are kept, and only the rest is asked again.
        Err(e) => {
            let pairs = loose_pairs(body);
            if pairs.is_empty() { Err(trf!("the answer is not valid JSON: {e}", e)) } else { Ok(pairs) }
        }
    }
}

/// The `"<digits>": "<JSON string>"` pairs of a broken JSON object, in any order.
fn loose_pairs(body: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let b = body.as_bytes();
    let mut i = 0;
    while let Some(q) = body[i..].find('"').map(|q| q + i) {
        // A key: digits between quotes, then a colon.
        let digits = body[q + 1..].bytes().take_while(u8::is_ascii_digit).count();
        let after_key = q + 1 + digits;
        if digits == 0 || b.get(after_key) != Some(&b'"') {
            i = q + 1;
            continue;
        }
        let mut j = after_key + 1;
        while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
            j += 1;
        }
        if b.get(j) != Some(&b':') {
            i = after_key + 1;
            continue;
        }
        j += 1;
        while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
            j += 1;
        }
        if b.get(j) != Some(&b'"') {
            i = j.max(after_key + 1);
            continue;
        }
        // The value: a JSON string up to its unescaped closing quote.
        let mut k = j + 1;
        while k < b.len() && b[k] != b'"' {
            k += if b[k] == b'\\' { 2 } else { 1 };
        }
        if k >= b.len() {
            break;
        }
        if let Ok(v) = serde_json::from_str::<String>(&body[j..=k]) {
            out.insert(body[q + 1..after_key].to_string(), v);
        }
        i = k + 1;
    }
    out
}

/// `curl` with its window hidden on Windows.
fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-sS", "-A", concat!("razdor-translate/", env!("CARGO_PKG_VERSION"))]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// A request to `url`: `body` (JSON) posted, or a GET without one. Returns the status and
/// the body.
fn request(s: &Settings, url: &str, body: Option<&Value>) -> Result<(u32, String), String> {
    // The key goes in a curl config file only this user can read.
    let dir = std::env::temp_dir();
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let conf = dir.join(format!("razdor-translate-{}-{n}.curl", std::process::id()));
    let mut lines = format!("url = \"{}\"\nheader = \"Content-Type: application/json\"\n", url.replace('"', "%22"));
    if !s.api_key.trim().is_empty() {
        lines.push_str(&format!("header = \"Authorization: Bearer {}\"\n", s.api_key.trim().replace('"', "")));
    }
    write_private(&conf, lines.as_bytes()).map_err(|e| format!("{}: {e}", conf.display()))?;
    let mut cmd = curl();
    cmd.args(["--max-time", &s.timeout.to_string(), "-w", "\n%{http_code}", "-K"]).arg(&conf);
    if body.is_some() {
        cmd.args(["--data-binary", "@-"]);
    }
    let mut run = || -> Result<std::process::Output, String> {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        if let (Some(mut stdin), Some(b)) = (child.stdin.take(), body) {
            stdin.write_all(b.to_string().as_bytes()).map_err(|e| format!("curl: {e}"))?;
        }
        child.wait_with_output().map_err(|e| format!("curl: {e}"))
    };
    let out = run();
    let _ = std::fs::remove_file(&conf);
    let out = out?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    let code: u32 = code.trim().parse().unwrap_or(0);
    if code == 0 {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() { "no answer from the server".into() } else { why });
    }
    Ok((code, body.to_string()))
}

fn api_error(code: u32, body: &str) -> String {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").or_else(|| v.get("error")).map(|m| m.as_str().map_or(m.to_string(), str::to_string)));
    format!("HTTP {code}: {}", msg.unwrap_or_else(|| body.chars().take(300).collect()))
}

/// Servers that answered only without `reasoning_effort` and `response_format`.
static PLAIN_SERVERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// One chat completion: the model's answer text.
pub fn chat(s: &Settings, system: &str, user: &str) -> Result<String, String> {
    let mut body = json!({
        "model": s.model.trim(),
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "temperature": s.temperature,
        "stream": false,
    });
    if s.json_mode {
        body["response_format"] = json!({"type": "json_object"});
    }
    let effort = s.reasoning_effort.trim();
    if !effort.is_empty() {
        body["reasoning_effort"] = json!(effort);
    }
    let url = s.url("chat/completions");
    // A server that refused the optional ones once (LM Studio knows no `json_object`) is not
    // asked with them again in this run: every batch would cost two requests.
    if PLAIN_SERVERS.lock().unwrap().contains(&url) {
        let obj = body.as_object_mut().expect("a JSON object");
        obj.remove("reasoning_effort");
        obj.remove("response_format");
    }
    let (mut code, mut text) = request(s, &url, Some(&body))?;
    // A server that does not know an option: once more without the optional ones.
    if code == 400 && (body.get("reasoning_effort").is_some() || body.get("response_format").is_some()) {
        let obj = body.as_object_mut().expect("a JSON object");
        obj.remove("reasoning_effort");
        obj.remove("response_format");
        (code, text) = request(s, &url, Some(&body))?;
        if code == 200 {
            PLAIN_SERVERS.lock().unwrap().push(url.clone());
        }
    }
    if code != 200 {
        return Err(api_error(code, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("unreadable answer: {e}"))?;
    let choice = v.pointer("/choices/0").ok_or_else(|| api_error(code, &text))?;
    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        return Err("the answer was cut off (the context is too small for this batch)".into());
    }
    choice.pointer("/message/content").and_then(Value::as_str).map(str::to_string).ok_or_else(|| "the answer has no text".into())
}

/// The models the server offers (`GET /models`): checks the address and the key.
pub fn models(s: &Settings) -> Result<Vec<String>, String> {
    let (code, text) = request(s, &s.url("models"), None)?;
    if code != 200 {
        return Err(api_error(code, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("unreadable answer: {e}"))?;
    let list = v.get("data").and_then(Value::as_array).ok_or("the server lists no models")?;
    Ok(list.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_numbers_texts_and_hides_tokens() {
        let p = user_prompt(&["Привет, #HERONAME", "Да"], &[("Иван".into(), "Ivan".into())], "English", "");
        assert!(p.starts_with("Glossary (Russian = English):\nИван = Ivan\n\nTranslate into English:\n"));
        let json = &p[p.find('{').unwrap()..];
        let v: Value = serde_json::from_str(json).unwrap();
        assert_eq!(v, json!({"1": "Привет, {0}", "2": "Да"}));
    }

    #[test]
    fn reads_the_json_out_of_an_answer() {
        let got = parse_answer("Sure!\n```json\n{\"1\": \"Hello\", \"2\": \"Yes\", \"3\": 5}\n```").unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got["1"], "Hello");
        assert!(parse_answer("no json here").is_err());
        // One broken entry: the others are kept.
        let broken = "{\n \"1\": \"Hello, \\\"friend\\\"\",\n \"2\": 12-3,\n \"3\": \"Yes: {0}\"\n}";
        let got = parse_answer(broken).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got["1"], "Hello, \"friend\"");
        assert_eq!(got["3"], "Yes: {0}");
        assert!(parse_answer("{\"1\": ").is_err());
        // Each failure says what the answer was.
        assert_eq!(parse_answer(" \n").unwrap_err(), "the answer is empty");
        assert_eq!(parse_answer("no json here").unwrap_err(), "no JSON object in the answer");
        assert!(parse_answer("{oops}").unwrap_err().starts_with("the answer is not valid JSON: "));
    }

    #[test]
    fn the_key_file_is_private_and_never_reused() {
        let path = std::env::temp_dir().join(format!("razdor-translate-test-{}.curl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        write_private(&path, b"x").unwrap();
        assert!(write_private(&path, b"y").is_err(), "an existing file is not reused");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_bare_server_address_gets_the_openai_path() {
        assert_eq!(api_base("http://127.0.0.1:1234"), "http://127.0.0.1:1234/v1");
        assert_eq!(api_base(" http://127.0.0.1:1234/ "), "http://127.0.0.1:1234/v1");
        assert_eq!(api_base("http://localhost:11434/v1"), "http://localhost:11434/v1");
        assert_eq!(api_base("https://openrouter.ai/api/v1/"), "https://openrouter.ai/api/v1");
    }

    #[test]
    fn the_style_follows_the_rules() {
        let plain = Settings::default();
        assert_eq!(system_prompt(&plain), rules("English", false));
        let styled = Settings { style: " Archaic, like Tolkien ".into(), notes: "Ivan is a king.".into(), ..Settings::default() };
        let p = system_prompt(&styled);
        assert!(p.ends_with("Archaic, like Tolkien") && p.contains("Ivan is a king."));
        // The player's style replaces the default tone instead of trailing after it.
        assert!(!p.contains("the tone of the original") && p.contains("THE STYLE OF THIS TRANSLATION"));
        let u = user_prompt(&["Да"], &[], "German", " Archaic ");
        assert!(u.contains("Translate into German:") && u.ends_with("Write every text in this style: Archaic"));
    }

    #[test]
    fn russian_into_russian_is_a_rewrite_in_the_style() {
        assert!(is_source_language("Russian") && is_source_language(" русский ") && !is_source_language("Belarusian"));
        let s = Settings { language: "Russian".into(), style: "смешной, ссср".into(), ..Settings::default() };
        let p = system_prompt(&s);
        assert!(p.contains("Rewrite each text in Russian in the player's style") && p.contains("Never return a text unchanged"));
        assert!(!p.contains("You translate"));
        assert!(user_prompt(&["Да"], &[], "Russian", "смешной").contains("Rewrite in Russian:"));
    }

    #[test]
    fn the_rules_name_the_language_and_its_letters() {
        let en = rules("English", false);
        assert!(en.contains("into English for its English edition") && en.contains("plain ASCII") && en.contains("{0}, {1}"));
        let de = rules("German", false);
        assert!(de.contains("into German") && de.contains("Windows-1251") && de.contains("No Cyrillic"));
        assert!(!rules("Ukrainian", false).contains("No Cyrillic"));
        assert!(rules("  ", false).contains("into English"));
        let hi = rules("Hindi", true);
        assert!(hi.contains("own script") && !hi.contains("Windows-1251"));
    }

    #[test]
    fn batches_grow_with_the_context() {
        let s = |context| Settings { context, ..Settings::default() };
        assert_eq!(s(8192).batch_chars(), 3276);
        assert_eq!(s(131072).batch_chars(), 6000);
        assert_eq!(s(512).batch_chars(), 400);
        assert_eq!(s(8192).batch_items(), 30);
    }
}
