//! The typed identities: the install id (analytics), the account key and the link code (cloud).
//! Each serializes as its plain string. A struct that holds one must call its `validate` too: serde
//! alone does not check the shape.

use serde::{Deserialize, Serialize};

use crate::check;
use crate::error::{WireError, WireResult};
use crate::limits;
use crate::Validate;

/// A random install id, made at the game's first start (not tied to the device).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstallId(String);

impl InstallId {
    pub fn parse(s: &str) -> WireResult<InstallId> {
        let id = InstallId(s.to_string());
        id.validate()?;
        Ok(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Validate for InstallId {
    fn validate(&self) -> WireResult {
        check::hex("install_id", &self.0, limits::INSTALL_ID_LEN, limits::INSTALL_ID_LEN)
    }
}

/// A cloud account's secret key (the game keeps it; every keyed request carries it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountKey(String);

impl AccountKey {
    pub fn parse(s: &str) -> WireResult<AccountKey> {
        let key = AccountKey(s.to_string());
        key.validate()?;
        Ok(key)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Validate for AccountKey {
    fn validate(&self) -> WireResult {
        check::hex("account_key", &self.0, limits::ACCOUNT_KEY_MIN_LEN, limits::ACCOUNT_KEY_MAX_LEN)
    }
}

/// A link code as shown on screen: `XXXX-XXXX` over [`limits::LINK_ALPHABET`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LinkCode(String);

impl LinkCode {
    /// Reads a typed code: spaces and case are ignored, the dash is optional.
    pub fn parse(s: &str) -> WireResult<LinkCode> {
        let chars: Vec<char> = s
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if chars.len() != 8 {
            return Err(WireError::Invalid { field: "link_code", reason: "a code has 8 characters" });
        }
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[4..].iter().collect();
        let text = format!("{head}-{tail}");
        let code = LinkCode(text);
        code.validate()?;
        Ok(code)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Validate for LinkCode {
    fn validate(&self) -> WireResult {
        let b = self.0.as_bytes();
        let shape_ok = b.len() == 9 && b[4] == b'-';
        if !shape_ok {
            return Err(WireError::Invalid { field: "link_code", reason: "not XXXX-XXXX" });
        }
        let ok = b
            .iter()
            .enumerate()
            .all(|(i, &c)| i == 4 || limits::LINK_ALPHABET.as_bytes().contains(&c));
        if !ok {
            return Err(WireError::Invalid { field: "link_code", reason: "character outside the code alphabet" });
        }
        Ok(())
    }
}
