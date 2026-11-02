//! `texts/<lang>.toml`: the display texts of a package, one file per language. Nested tables
//! give dotted keys: `[unit.knight] name = "…"` is the key `unit.knight.name`.

use std::collections::BTreeMap;

/// Flattens a parsed texts file into dotted keys. A value that is not a string is an error.
pub fn flatten(table: &toml::Table) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    flatten_into(table, "", &mut out)?;
    Ok(out)
}

fn flatten_into(table: &toml::Table, prefix: &str, out: &mut BTreeMap<String, String>) -> Result<(), String> {
    for (key, value) in table {
        let full = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        match value {
            toml::Value::String(s) => {
                out.insert(full, s.clone());
            }
            toml::Value::Table(t) => flatten_into(t, &full, out)?,
            other => return Err(format!("`{full}` must be a text (a string), not {}", other.type_str())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_tables_make_dotted_keys() {
        let t: toml::Table = toml::from_str("title = \"A\"\n[unit.knight]\nname = \"K\"\ndescription = \"D\"\n").unwrap();
        let flat = flatten(&t).unwrap();
        assert_eq!(flat.get("title").map(String::as_str), Some("A"));
        assert_eq!(flat.get("unit.knight.name").map(String::as_str), Some("K"));
        assert_eq!(flat.get("unit.knight.description").map(String::as_str), Some("D"));
        assert_eq!(flat.len(), 3);
    }

    #[test]
    fn a_number_is_not_a_text() {
        let t: toml::Table = toml::from_str("count = 3\n").unwrap();
        assert!(flatten(&t).unwrap_err().contains("count"));
    }
}
