//! `units/*.toml` and `items/*.toml`: the content a package adds or changes. Each file holds
//! `[[unit]]` (or `[[item]]`) tables; a table is based on base content (`base`) and changes
//! the stats it names. Names and descriptions are text keys.

use serde::{Deserialize, Deserializer};
use toml::Spanned;

use super::refs::Ref;

fn de_ref<'de, D: Deserializer<'de>>(d: D) -> Result<Ref, D::Error> {
    let text = String::deserialize(d)?;
    Ref::parse(&text).map_err(serde::de::Error::custom)
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitFile {
    #[serde(default)]
    pub unit: Vec<Spanned<UnitSpec>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitSpec {
    pub id: String,
    #[serde(deserialize_with = "de_ref")]
    pub base: Ref,
    /// A text key.
    pub name: String,
    /// A text key.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub cost: Option<i32>,
    #[serde(default)]
    pub hits: Option<i32>,
    #[serde(default)]
    pub attack_blow: Option<i32>,
    #[serde(default)]
    pub attack_shot: Option<i32>,
    #[serde(default)]
    pub magic_power: Option<i32>,
    #[serde(default)]
    pub defence_blow: Option<i32>,
    #[serde(default)]
    pub defence_shot: Option<i32>,
    #[serde(default)]
    pub protect_life: Option<i32>,
    #[serde(default)]
    pub protect_death: Option<i32>,
    #[serde(default)]
    pub protect_elemental: Option<i32>,
    #[serde(default)]
    pub initiative: Option<i32>,
    #[serde(default)]
    pub manevres: Option<i32>,
    #[serde(default)]
    pub regen: Option<i32>,
    #[serde(default)]
    pub vampirism: Option<i32>,
    #[serde(default)]
    pub evasion: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFile {
    #[serde(default)]
    pub item: Vec<Spanned<ItemSpec>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSpec {
    pub id: String,
    #[serde(deserialize_with = "de_ref")]
    pub base: Ref,
    /// A text key.
    pub name: String,
    /// A text key.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub cost: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_table_reads_its_refs_and_overrides() {
        let f: UnitFile = toml::from_str(
            "[[unit]]\nid = \"a\"\nbase = \"dt:knight\"\nname = \"unit.a.name\"\nhits = 9\n",
        )
        .unwrap();
        let u = f.unit[0].get_ref();
        assert_eq!(u.base, Ref::Dt { kind: super::super::refs::DtKind::Unit, alias: "knight".into() });
        assert_eq!(u.hits, Some(9));
        assert_eq!(u.attack_blow, None);
    }

    #[test]
    fn unknown_fields_and_bad_refs_are_errors() {
        assert!(toml::from_str::<UnitFile>("[[unit]]\nid = \"a\"\nbase = \"dt:knight\"\nname = \"n\"\nspeed = 2\n").is_err());
        let bad = toml::from_str::<UnitFile>("[[unit]]\nid = \"a\"\nbase = \"Knight\"\nname = \"n\"\n");
        assert!(bad.unwrap_err().message().contains("lowercase"));
    }
}
