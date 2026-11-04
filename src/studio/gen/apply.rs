//! Adds a generated map to a package as one undo step: its two files (`maps/<id>/map.toml`
//! and `terrain.png`), its text keys in every `texts/<lang>.toml`, and, when asked, its entry
//! in `campaign.toml`. Undo removes the files and takes the keys and the entry back out.

use crate::studio::doc::{table, DocEdit};
use crate::studio::Workspace;
use toml_edit::Value;

use super::to_package::PackageMap;

/// A map id the package accepts: `[a-z0-9-]+`, the folder name.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Adds `map` as `maps/<id>/` of the open package as one undo step named `New map <id>`.
/// `in_campaign` also appends it to `campaign.toml` (an error when the package has none). A
/// failure leaves the package as it was.
pub fn add_map(ws: &mut Workspace, id: &str, map: &PackageMap, in_campaign: bool) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("{id}: a map id is lowercase letters, digits and dashes"));
    }
    let folder = format!("maps/{id}/");
    if ws.entries.iter().any(|e| e.starts_with(&folder)) {
        return Err(format!("maps/{id} already exists"));
    }
    let text_files: Vec<String> = ws.entries.iter().filter(|e| e.starts_with("texts/") && e.ends_with(".toml")).cloned().collect();
    if text_files.is_empty() {
        return Err("the package has no texts/<lang>.toml to name the map in".into());
    }
    if in_campaign && !ws.entries.iter().any(|e| e == "campaign.toml") {
        return Err("the package has no campaign.toml".into());
    }
    let label = format!("New map {id}");
    ws.begin_group();
    let result = (|| -> Result<(), String> {
        ws.create_files(
            vec![(format!("{folder}map.toml"), map.toml.clone().into_bytes()), (format!("{folder}terrain.png"), map.png.clone())],
            &label,
        )?;
        let mut edits = Vec::new();
        for file in &text_files {
            edits.push((file.clone(), text_edits(&map.texts)));
        }
        ws.apply_docs(&label, edits)?;
        if in_campaign {
            ws.apply_doc("campaign.toml", &label, &[DocEdit::Push { path: Vec::new(), key: "map".into(), fields: vec![("id".into(), Value::from(id))] }])?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            ws.end_group(&label);
            Ok(())
        }
        Err(e) => {
            ws.cancel_group();
            Err(e)
        }
    }
}

/// The edits that set each text key: a key `gen.map.title` is `title` in the table `[gen.map]`.
fn text_edits(texts: &[(String, String)]) -> Vec<DocEdit> {
    texts
        .iter()
        .map(|(key, text)| {
            let mut parts: Vec<&str> = key.split('.').collect();
            let leaf = parts.pop().unwrap_or_default().to_string();
            DocEdit::Set { path: parts.iter().map(|p| table(p)).collect(), key: leaf, value: Value::from(text.as_str()) }
        })
        .collect()
}

/// A copy of the package folder `from` in a new folder of the system's temporary directory (the
/// snapshot's package, so a snapshot never writes to the package it shows).
pub fn copy_package(from: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let to = std::env::temp_dir().join(format!("razdor-gen-copy-{}-{nanos}", std::process::id()));
    copy_tree(from, &to).map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
    Ok(to)
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}
