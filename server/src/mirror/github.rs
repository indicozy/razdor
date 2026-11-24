//! What the mirror reads from GitHub: the releases API's JSON, the catalog's addresses, the
//! SHA256SUMS file. Pure functions, tested with fixtures (no network).

use serde_json::{json, Value};

use super::config::{glob_match, MirrorConfig};

/// The owner's repositories the mirror copies. Anything else has no own copy.
pub const OWNER: &str = "indicozy";
pub const REPOS: [&str; 2] = ["razdor", "razdor-community"];

/// The old fixed names that 1.5.2+ clients do not read when a versioned file exists.
const OLD_FIXED: [&str; 5] = ["razdor", "razdor.exe", "razdor-macos", "razdor.apk", "razdor.ipa"];

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    /// GitHub's `browser_download_url`.
    pub url: String,
    pub size: u64,
    /// The SHA-256 from the API's `digest`, lower-case hex; `None` for an older asset without one.
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
    /// The release object as GitHub sent it (the feed keeps its shape).
    pub raw: Value,
}

/// `sha256:<hex>` to `<hex>`, lower-case; anything else is `None`.
pub fn digest_hex(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(hex.to_ascii_lowercase())
    } else {
        None
    }
}

/// The releases of a list reply, or the one release of a `releases/tags/<tag>` reply.
pub fn parse_releases(body: &[u8]) -> Result<Vec<Release>, String> {
    let list: Value = serde_json::from_slice(body).map_err(|e| format!("releases: {e}"))?;
    let items: Vec<&Value> = match &list {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![&list],
        _ => return Err("releases: neither a list nor a release".into()),
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let tag = item.get("tag_name").and_then(Value::as_str).ok_or("releases: a release has no tag_name")?;
        let assets = item
            .get("assets")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(parse_asset).collect())
            .unwrap_or_default();
        out.push(Release {
            tag: tag.to_string(),
            draft: item.get("draft").and_then(Value::as_bool).unwrap_or(false),
            prerelease: item.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
            assets,
            raw: item.clone(),
        });
    }
    Ok(out)
}

fn parse_asset(a: &Value) -> Option<Asset> {
    Some(Asset {
        name: a.get("name")?.as_str()?.to_string(),
        url: a.get("browser_download_url")?.as_str()?.to_string(),
        size: a.get("size").and_then(Value::as_u64).unwrap_or(0),
        digest: a.get("digest").and_then(Value::as_str).and_then(digest_hex),
    })
}

/// The newest `keep` releases that are neither drafts nor pre-releases (the client skips both too).
pub fn newest_published(list: &[Release], keep: usize) -> Vec<&Release> {
    list.iter()
        .filter(|r| !r.draft && !r.prerelease && r.tag != super::site::SITE_TAG)
        .take(keep)
        .collect()
}

/// The release's version: the tag without its `v`.
pub fn version_of(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// The assets the mirror keeps of a release: not excluded, the old fixed names out when versioned
/// files exist (with `skip_unversioned`), and SHA256SUMS always.
pub fn wanted<'a>(rel: &'a Release, cfg: &MirrorConfig) -> Vec<&'a Asset> {
    let version = version_of(&rel.tag);
    let has_versioned = rel.assets.iter().any(|a| a.name.contains(version));
    rel.assets
        .iter()
        .filter(|a| !cfg.exclude.iter().any(|p| glob_match(p, &a.name)))
        .filter(|a| {
            !(cfg.skip_unversioned
                && has_versioned
                && !a.name.contains(version)
                && OLD_FIXED.iter().any(|f| f.eq_ignore_ascii_case(&a.name)))
        })
        .collect()
}

/// The SHA-256 of each name in a SHA256SUMS file (`<hex>  <name>` lines, `*name` for binary mode).
pub fn parse_sums(body: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(body);
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hex = digest_hex(&format!("sha256:{}", parts.next()?))?;
            let name = parts.next()?.trim_start_matches('*');
            Some((name.to_string(), hex))
        })
        .collect()
}

/// `https://indicozy.github.io/<repo>/<path>` and `https://github.com/indicozy/<repo>/<path>` map to the
/// own copy `<own_base>/indicozy/<repo>/<path>` or `<own_base>/pages/<repo>/<path>`. Only the owner's
/// two repositories have an own copy.
pub fn own_copy(url: &str, own_base: &str) -> Option<String> {
    let own_base = own_base.trim_end_matches('/');
    if let Some(rest) = url.strip_prefix("https://github.com/indicozy/") {
        let repo = rest.split('/').next()?;
        return REPOS.contains(&repo).then(|| format!("{own_base}/{OWNER}/{rest}"));
    }
    if let Some(rest) = url.strip_prefix("https://indicozy.github.io/") {
        let repo = rest.split('/').next()?;
        return REPOS.contains(&repo).then(|| format!("{own_base}/pages/{rest}"));
    }
    None
}

/// The release a GitHub download address names: `(repo, tag)` for `.../<owner>/<repo>/releases/download/<tag>/<name>`.
pub fn release_of_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://github.com/indicozy/")?;
    let mut parts = rest.split('/');
    let repo = parts.next()?;
    if parts.next()? != "releases" || parts.next()? != "download" {
        return None;
    }
    let tag = parts.next()?;
    REPOS.contains(&repo).then(|| (repo.to_string(), tag.to_string()))
}

/// The tag in a redirect's `Location` (`.../releases/download/<tag>/index.json`).
pub fn tag_from_location(location: &str) -> Option<String> {
    let (_, after) = location.split_once("/releases/download/")?;
    let tag = after.split('/').next()?;
    (!tag.is_empty()).then(|| tag.to_string())
}

/// The release tags a catalog names for files it needs: `game.url` and `ad.image`, as GitHub addresses or
/// as the own copies (a rewritten catalog names its own copies).
pub fn referenced_tags(index: &[u8], own_base: &str) -> Vec<(String, String)> {
    let Ok(v) = serde_json::from_slice::<Value>(index) else {
        return Vec::new();
    };
    let own_prefix = format!("{}/{OWNER}/", own_base.trim_end_matches('/'));
    let mut out = Vec::new();
    let urls = [v.pointer("/game/url"), v.pointer("/ad/image")];
    for url in urls.into_iter().flatten().filter_map(Value::as_str) {
        let github = match url.strip_prefix(&own_prefix) {
            Some(rest) => format!("https://github.com/{OWNER}/{rest}"),
            None => url.to_string(),
        };
        if let Some(r) = release_of_url(&github) {
            out.push(r);
        }
    }
    out
}

/// The `schema` a config must have (the same rule as the game's `remote_config::Config::parse`: schema 1).
pub fn check_config(body: &[u8]) -> Result<(), String> {
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("config: {e}"))?;
    match v.get("schema").and_then(Value::as_u64) {
        Some(1) => Ok(()),
        other => Err(format!("config: schema {other:?}, want 1")),
    }
}

/// The app's update feed: the kept published releases, each with its wanted assets only, the
/// addresses GitHub's (the client expands them to the own copy itself).
pub fn feed(releases: &[&Release], wanted_of: impl Fn(&Release) -> Vec<Asset>) -> Value {
    let items: Vec<Value> = releases
        .iter()
        .map(|r| {
            let assets: Vec<Value> = wanted_of(r)
                .iter()
                .map(|a| {
                    json!({
                        "name": a.name,
                        "size": a.size,
                        "digest": a.digest.as_ref().map(|d| format!("sha256:{d}")),
                        "browser_download_url": a.url,
                    })
                })
                .collect();
            let mut obj = r.raw.clone();
            if let Some(map) = obj.as_object_mut() {
                map.insert("assets".into(), Value::Array(assets));
            }
            obj
        })
        .collect();
    Value::Array(items)
}

// --- The catalog's own copy (option B: game.url rewritten, the old address first in game.mirrors) ---

/// The byte span `[start, end)` of the value at `path` (object keys) in `src`, or `None`.
pub fn value_span(src: &str, path: &[&str]) -> Option<(usize, usize)> {
    let b = src.as_bytes();
    let mut pos = skip_ws(b, 0);
    let mut span = None;
    for key in path {
        if *b.get(pos)? != b'{' {
            return None;
        }
        pos += 1;
        loop {
            pos = skip_ws(b, pos);
            match *b.get(pos)? {
                b'}' => return None,
                b',' => {
                    pos += 1;
                    continue;
                }
                b'"' => {}
                _ => return None,
            }
            let key_end = skip_string(b, pos)?;
            let found = &src[pos + 1..key_end - 1] == *key;
            pos = skip_ws(b, key_end);
            if *b.get(pos)? != b':' {
                return None;
            }
            pos = skip_ws(b, pos + 1);
            let value_start = pos;
            let value_end = skip_value(b, pos)?;
            if found {
                span = Some((value_start, value_end));
                pos = value_start;
                break;
            }
            pos = value_end;
        }
    }
    let (s, e) = span?;
    Some((s, e))
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// The index just past the string that starts at `i` (a `"`).
fn skip_string(b: &[u8], i: usize) -> Option<usize> {
    if *b.get(i)? != b'"' {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// The index just past the JSON value that starts at `i`.
fn skip_value(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => skip_string(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => j = skip_string(b, j)?,
                    b'{' | b'[' => {
                        depth += 1;
                        j += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        j += 1;
                        if depth == 0 {
                            return Some(j);
                        }
                    }
                    _ => j += 1,
                }
            }
            None
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            Some(j)
        }
    }
}

/// The catalog with `game.url` pointing at the own copy and GitHub's address first in `game.mirrors`.
/// Every other byte is kept; the result is parsed again and compared with the input (it must differ only
/// in those two keys), so a catalog the rewrite cannot read is refused.
pub fn rewrite_catalog(index: &[u8], own_base: &str) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(index).map_err(|_| "catalog: not UTF-8".to_string())?;
    let (us, ue) = value_span(text, &["game", "url"]).ok_or("catalog: no game.url")?;
    let url: String = serde_json::from_str(&text[us..ue]).map_err(|e| format!("catalog: game.url: {e}"))?;
    let own = own_copy(&url, own_base).ok_or("catalog: game.url is not a razdor-community address")?;
    let url_json = serde_json::to_string(&url).map_err(|e| e.to_string())?;
    let own_json = serde_json::to_string(&own).map_err(|e| e.to_string())?;

    // Edits as (start, end, text), applied from the end of the file so the earlier spans stay valid.
    let mut edits: Vec<(usize, usize, String)> = vec![(us, ue, own_json)];
    match value_span(text, &["game", "mirrors"]) {
        Some((ms, me)) => {
            let existing: Value = serde_json::from_str(&text[ms..me]).map_err(|e| format!("catalog: mirrors: {e}"))?;
            let list = existing.as_array().ok_or("catalog: game.mirrors is not a list")?;
            if !list.iter().any(|v| v.as_str() == Some(url.as_str())) {
                let empty = list.is_empty();
                edits.push((ms + 1, ms + 1, format!("{url_json}{}", if empty { "" } else { "," })));
            }
        }
        None => edits.push((ue, ue, format!(",\"mirrors\":[{url_json}]"))),
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = text.to_string();
    for (s, e, t) in edits {
        out.replace_range(s..e, &t);
    }

    // The check: the same document with game.url and game.mirrors removed, before and after.
    let before: Value = serde_json::from_str(text).map_err(|e| format!("catalog: {e}"))?;
    let after: Value = serde_json::from_str(&out).map_err(|e| format!("catalog rewrite: {e}"))?;
    if after.pointer("/game/url").and_then(Value::as_str) != Some(own.as_str()) {
        return Err("catalog rewrite: game.url not set".into());
    }
    if !after.pointer("/game/mirrors").and_then(Value::as_array).is_some_and(|l| l.iter().any(|v| v.as_str() == Some(url.as_str()))) {
        return Err("catalog rewrite: GitHub's address missing from game.mirrors".into());
    }
    let strip = |mut v: Value| {
        if let Some(g) = v.get_mut("game").and_then(Value::as_object_mut) {
            g.remove("url");
            g.remove("mirrors");
        }
        v
    };
    if strip(before) != strip(after) {
        return Err("catalog rewrite: other keys changed".into());
    }
    Ok(out.into_bytes())
}
