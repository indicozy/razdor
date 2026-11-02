//! `package.toml`: the package's id, version, licence and what it builds on.

use serde::Deserialize;

/// The format version this engine reads.
pub const FORMAT: u32 = 1;

/// Base packages a package may name.
pub const BASES: [&str; 2] = ["community-update", "evolution"];

/// The rulesets a package may name.
pub const RULESETS: [&str; 1] = ["dt"];

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub format: u32,
    /// A text key (`texts/<lang>.toml`).
    pub title: String,
    /// A text key.
    #[serde(default)]
    pub description: Option<String>,
    pub author: String,
    pub license: String,
    #[serde(default = "default_base")]
    pub base: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default = "default_rules")]
    pub rules: String,
}

fn default_base() -> String {
    "community-update".into()
}

fn default_rules() -> String {
    "dt".into()
}

/// One entry of `requires`: `<package-id>@<version-req>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub package: String,
    pub req: String,
}

/// Parses `id@req`. The version requirement is `*`, or an operator (`^`, `~`, `=`, `>`, `>=`,
/// `<`, `<=`; `^` by default) and one to three numbers.
pub fn parse_requirement(text: &str) -> Result<Requirement, String> {
    let (package, req) = text
        .split_once('@')
        .ok_or_else(|| format!("`{text}`: write it as id@version, e.g. elves-pack@^1.0"))?;
    if !super::refs::valid_package_id(package) {
        return Err(format!("`{package}` is not a package id: use lowercase letters, digits and - only"));
    }
    if !valid_req(req) {
        return Err(format!("`{req}` is not a version requirement: use *, or an operator and a version such as ^1.0"));
    }
    Ok(Requirement { package: package.to_string(), req: req.to_string() })
}

fn valid_req(req: &str) -> bool {
    if req == "*" {
        return true;
    }
    let ops = [">=", "<=", "^", "~", "=", ">", "<"];
    let rest = ops.iter().find_map(|op| req.strip_prefix(op)).unwrap_or(req);
    let parts: Vec<&str> = rest.split('.').collect();
    (1..=3).contains(&parts.len())
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `x.y.z` with numbers only.
pub fn valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements() {
        assert_eq!(parse_requirement("elves-pack@^1.0").unwrap(), Requirement { package: "elves-pack".into(), req: "^1.0".into() });
        assert!(parse_requirement("elves-pack@*").is_ok());
        assert!(parse_requirement("elves-pack@>=1.2.3").is_ok());
        assert!(parse_requirement("elves-pack").is_err());
        assert!(parse_requirement("Elves@^1.0").is_err());
        assert!(parse_requirement("elves@^1.x").is_err());
        assert!(parse_requirement("elves@^").is_err());
    }

    #[test]
    fn versions() {
        assert!(valid_version("0.1.0"));
        assert!(valid_version("12.0.3"));
        assert!(!valid_version("1.0"));
        assert!(!valid_version("1.0.0-beta"));
        assert!(!valid_version("v1.0.0"));
    }
}
