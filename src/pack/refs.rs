//! References in a package: `dt:` base content, this package's ids and other packages' ids.
//!
//! Grammar (ids are `[a-z0-9_]+`, package ids `[a-z0-9-]+`):
//! - `dt:<alias>` is a unit, `dt:unit/<alias>` a unit, `dt:item/<alias>` an item,
//!   `dt:spell/<alias>` a spell (`dt:` aliases are listed in `data/pack/dt-aliases.toml`);
//! - `<id>` is an id of this package;
//! - `<package>:<id>` is an id of another package listed in `requires`.

use std::fmt;

/// The kind of base content a `dt:` reference names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DtKind {
    Unit,
    Item,
    Spell,
}

impl DtKind {
    /// The kind word in a reference (`unit` in `dt:unit/knight`).
    pub fn word(self) -> &'static str {
        match self {
            DtKind::Unit => "unit",
            DtKind::Item => "item",
            DtKind::Spell => "spell",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ref {
    Dt { kind: DtKind, alias: String },
    Local { id: String },
    Package { package: String, id: String },
}

impl Ref {
    /// Parses a reference; the error is a sentence for the author.
    pub fn parse(text: &str) -> Result<Ref, String> {
        if text.is_empty() {
            return Err("empty reference".into());
        }
        if let Some(rest) = text.strip_prefix("dt:") {
            let (kind, alias) = match rest.split_once('/') {
                None => (DtKind::Unit, rest),
                Some(("unit", a)) => (DtKind::Unit, a),
                Some(("item", a)) => (DtKind::Item, a),
                Some(("spell", a)) => (DtKind::Spell, a),
                Some((word, _)) => {
                    return Err(format!("`dt:{word}/…` is not a kind: use dt:unit/, dt:item/ or dt:spell/"));
                }
            };
            if !valid_id(alias) {
                return Err(format!("`{text}`: the alias must be lowercase letters, digits and _"));
            }
            return Ok(Ref::Dt { kind, alias: alias.to_string() });
        }
        match text.split_once(':') {
            None => {
                if valid_id(text) {
                    Ok(Ref::Local { id: text.to_string() })
                } else {
                    Err(format!("`{text}` is not an id: use lowercase letters, digits and _ only"))
                }
            }
            Some((package, id)) => {
                if !valid_package_id(package) {
                    return Err(format!("`{package}` is not a package id: use lowercase letters, digits and - only"));
                }
                if package == "dt" {
                    return Err("`dt` is reserved for the base game: write dt:<alias>".into());
                }
                if !valid_id(id) {
                    return Err(format!("`{text}`: the id must be lowercase letters, digits and _"));
                }
                Ok(Ref::Package { package: package.to_string(), id: id.to_string() })
            }
        }
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ref::Dt { kind: DtKind::Unit, alias } => write!(f, "dt:{alias}"),
            Ref::Dt { kind, alias } => write!(f, "dt:{}/{alias}", kind.word()),
            Ref::Local { id } => write!(f, "{id}"),
            Ref::Package { package, id } => write!(f, "{package}:{id}"),
        }
    }
}

impl TryFrom<String> for Ref {
    type Error = String;

    fn try_from(text: String) -> Result<Ref, String> {
        Ref::parse(&text)
    }
}

/// `[a-z0-9_]+`: the grammar of ids and `dt:` aliases.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `[a-z0-9-]+`: the grammar of package ids.
pub fn valid_package_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_grammar() {
        assert_eq!(Ref::parse("dt:knight"), Ok(Ref::Dt { kind: DtKind::Unit, alias: "knight".into() }));
        assert_eq!(Ref::parse("dt:unit/knight"), Ok(Ref::Dt { kind: DtKind::Unit, alias: "knight".into() }));
        assert_eq!(Ref::parse("dt:item/sword_2"), Ok(Ref::Dt { kind: DtKind::Item, alias: "sword_2".into() }));
        assert_eq!(Ref::parse("dt:spell/fireball"), Ok(Ref::Dt { kind: DtKind::Spell, alias: "fireball".into() }));
        assert_eq!(Ref::parse("dawn_knight"), Ok(Ref::Local { id: "dawn_knight".into() }));
        assert_eq!(Ref::parse("elves-pack:elf_archer"), Ok(Ref::Package { package: "elves-pack".into(), id: "elf_archer".into() }));
    }

    #[test]
    fn rejects_bad_references() {
        assert!(Ref::parse("").is_err());
        assert!(Ref::parse("Knight").unwrap_err().contains("lowercase"));
        assert!(Ref::parse("dt:").is_err());
        assert!(Ref::parse("dt:Knight").is_err());
        assert!(Ref::parse("dt:building/castle").unwrap_err().contains("not a kind"));
        assert!(Ref::parse("dt:item/").is_err());
        assert!(Ref::parse("Elves:elf").is_err());
        assert!(Ref::parse("elves:Elf").is_err());
        assert!(Ref::parse("dt:x:y").is_err());
        assert!(Ref::parse("dt-pack:elf").is_ok());
        assert!(Ref::parse("a:b:c").is_err());
        assert!(Ref::parse("elves:").is_err());
        assert!(Ref::parse("dt:swordsman dt").is_err());
    }

    #[test]
    fn display_round_trips() {
        for text in ["dt:knight", "dt:item/relic", "dt:spell/fire", "x_1", "elves-pack:elf"] {
            assert_eq!(Ref::parse(text).unwrap().to_string(), text);
        }
        assert_eq!(Ref::parse("dt:unit/knight").unwrap().to_string(), "dt:knight");
    }
}
