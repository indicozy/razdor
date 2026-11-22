//! The cloud's typed calls (`razdor-wire` bodies over [`Net`]). Every body is validated both ways,
//! and every answer is mapped onto [`CloudError`]: the sync decides by the kind, the UI shows it.

use std::sync::Arc;

use razdor_wire::cloud::{
    AccountCreated, AccountNew, BenchReport, HubList, LinkIssued, LinkRedeem, LinkRedeemed, Marks, SaveEntry,
    SaveList, SaveMeta, SavePut, SyncedSettings,
};
use razdor_wire::{frame, paths, Validate, SCHEMA};
use serde::de::DeserializeOwned;

use super::http::{Net, Reply};
use super::pack::sha256_hex;

/// Why a cloud call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloudError {
    /// No answer: no network, a timeout, a throttle or a server error. Tried again later.
    Offline(String),
    /// The account's quota is full: «Облако заполнено, удалите старые сохранения».
    Full,
    /// Refused because of a conflict or a limit (a map's save count, a session that changed); the
    /// server's text.
    Conflict(String),
    /// No such save (it was removed on another device).
    NotFound,
    /// The key is missing or unknown (the account was deleted).
    Unauthorized,
    /// The server refused the request as malformed or unsupported (a newer body, for one).
    Refused(String),
    /// The answer did not check out (not valid, or a save whose hash differs).
    Bad(String),
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudError::Offline(why) => write!(f, "offline: {why}"),
            CloudError::Full => write!(f, "the cloud is full"),
            CloudError::Conflict(text) => write!(f, "conflict: {text}"),
            CloudError::NotFound => write!(f, "not found"),
            CloudError::Unauthorized => write!(f, "unauthorized"),
            CloudError::Refused(text) => write!(f, "refused: {text}"),
            CloudError::Bad(text) => write!(f, "bad answer: {text}"),
        }
    }
}

/// The API at `base`, with the account key (when there is one).
pub struct Api {
    net: Arc<dyn Net>,
    base: String,
    key: Option<String>,
}

impl Api {
    pub fn new(net: Arc<dyn Net>, base: &str, key: Option<String>) -> Api {
        Api { net, base: base.trim_end_matches('/').to_string(), key }
    }

    /// One call. Keyed calls fail as unauthorized without a key, before anything is sent.
    fn send(&self, method: &str, path: &str, body: Option<&[u8]>, keyed: bool) -> Result<Vec<u8>, CloudError> {
        let key = if keyed {
            Some(self.key.as_deref().ok_or(CloudError::Unauthorized)?)
        } else {
            None
        };
        let url = format!("{}{path}", self.base);
        let Reply { status, body: answer } = self.net.send(method, &url, key, body).map_err(CloudError::Offline)?;
        match status {
            200..=299 => Ok(answer),
            401 | 403 => Err(CloudError::Unauthorized),
            404 => Err(CloudError::NotFound),
            409 => Err(CloudError::Conflict(message(&answer))),
            413 => Err(CloudError::Full),
            408 | 429 | 500..=599 => Err(CloudError::Offline(format!("HTTP {status}"))),
            _ => Err(CloudError::Refused(message(&answer))),
        }
    }

    fn decode<T: DeserializeOwned + Validate>(body: &[u8]) -> Result<T, CloudError> {
        let value: T = serde_json::from_slice(body).map_err(|e| CloudError::Bad(e.to_string()))?;
        value.validate().map_err(|e| CloudError::Bad(e.to_string()))?;
        Ok(value)
    }

    fn json_body<T: serde::Serialize + Validate>(value: &T) -> Result<Vec<u8>, CloudError> {
        value.validate().map_err(|e| CloudError::Refused(e.to_string()))?;
        serde_json::to_vec(value).map_err(|e| CloudError::Refused(e.to_string()))
    }

    /// A new account: its key (the game keeps it; the server keeps only its hash).
    pub fn create_account(&self) -> Result<String, CloudError> {
        let body = Self::json_body(&AccountNew { schema: SCHEMA })?;
        let answer = self.send("POST", paths::ACCOUNT, Some(&body), false)?;
        let created: AccountCreated = Self::decode(&answer)?;
        Ok(created.key.as_str().to_string())
    }

    /// Deletes the account and every file of it.
    pub fn delete_account(&self) -> Result<(), CloudError> {
        self.send("DELETE", paths::ACCOUNT, None, true).map(|_| ())
    }

    /// A link code for a second device: valid for ten minutes, once.
    pub fn link_issue(&self) -> Result<LinkIssued, CloudError> {
        let answer = self.send("POST", paths::LINK, None, true)?;
        Self::decode(&answer)
    }

    /// The key a code gives (the second device then keeps it). A wrong or used code is refused.
    pub fn link_redeem(&self, code: &str) -> Result<String, CloudError> {
        let body = Self::json_body(&LinkRedeem { schema: SCHEMA, code: code.to_string() })?;
        let answer = self.send("POST", paths::LINK_REDEEM, Some(&body), false)?;
        let redeemed: LinkRedeemed = Self::decode(&answer)?;
        Ok(redeemed.key.as_str().to_string())
    }

    pub fn list_saves(&self) -> Result<SaveList, CloudError> {
        let answer = self.send("GET", paths::SAVES, None, true)?;
        Self::decode(&answer)
    }

    /// Stores one packed save (`packed` is the body; `meta` its size and hash).
    pub fn put_save(&self, meta: &SaveMeta, packed: &[u8]) -> Result<SavePut, CloudError> {
        meta.validate().map_err(|e| CloudError::Refused(e.to_string()))?;
        let framed = frame::encode(meta, packed).map_err(|e| CloudError::Refused(e.to_string()))?;
        let answer = self.send("PUT", paths::SAVES, Some(&framed), true)?;
        Self::decode(&answer)
    }

    /// One save: its metadata and packed body. The body must match the metadata's size and hash.
    pub fn get_save(&self, id: &str) -> Result<(SaveMeta, Vec<u8>), CloudError> {
        let answer = self.send("GET", &paths::save(id), None, true)?;
        let (meta, body): (SaveMeta, &[u8]) = frame::decode(&answer).map_err(|e| CloudError::Bad(e.to_string()))?;
        meta.validate().map_err(|e| CloudError::Bad(e.to_string()))?;
        if body.len() as u64 != meta.size || sha256_hex(body) != meta.sha256 {
            return Err(CloudError::Bad("the save's body differs from its header".into()));
        }
        Ok((meta, body.to_vec()))
    }

    pub fn delete_save(&self, id: &str) -> Result<(), CloudError> {
        self.send("DELETE", &paths::save(id), None, true).map(|_| ())
    }

    /// The account's marks, merged on the server with what is sent.
    pub fn put_marks(&self, marks: &Marks) -> Result<Marks, CloudError> {
        let body = Self::json_body(marks)?;
        let answer = self.send("PUT", paths::MARKS, Some(&body), true)?;
        Self::decode(&answer)
    }

    pub fn get_marks(&self) -> Result<Marks, CloudError> {
        let answer = self.send("GET", paths::MARKS, None, true)?;
        Self::decode(&answer)
    }

    /// The synced settings, merged on the server (the newest change of each key wins).
    pub fn put_settings(&self, settings: &SyncedSettings) -> Result<SyncedSettings, CloudError> {
        let body = Self::json_body(settings)?;
        let answer = self.send("PUT", paths::SETTINGS, Some(&body), true)?;
        Self::decode(&answer)
    }

    pub fn get_settings(&self) -> Result<SyncedSettings, CloudError> {
        let answer = self.send("GET", paths::SETTINGS, None, true)?;
        Self::decode(&answer)
    }

    /// Replaces the stored Hub list when it is not older (the server's rule).
    pub fn put_hub(&self, list: &HubList) -> Result<HubList, CloudError> {
        let body = Self::json_body(list)?;
        let answer = self.send("PUT", paths::HUB, Some(&body), true)?;
        Self::decode(&answer)
    }

    pub fn get_hub(&self) -> Result<HubList, CloudError> {
        let answer = self.send("GET", paths::HUB, None, true)?;
        Self::decode(&answer)
    }

    /// One performance test's report.
    pub fn post_bench(&self, report: &BenchReport) -> Result<(), CloudError> {
        let body = Self::json_body(report)?;
        self.send("POST", paths::BENCH, Some(&body), true).map(|_| ())
    }

    /// The account's last reports (the server keeps 50).
    pub fn get_bench(&self) -> Result<Vec<BenchReport>, CloudError> {
        let answer = self.send("GET", paths::BENCH, None, true)?;
        let reports: Vec<BenchReport> = serde_json::from_slice(&answer).map_err(|e| CloudError::Bad(e.to_string()))?;
        for r in &reports {
            r.validate().map_err(|e| CloudError::Bad(e.to_string()))?;
        }
        Ok(reports)
    }
}

/// The server's `error` text of a refusal, or empty.
fn message(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_default()
}

/// The saves of a list that belong to one map, kind and game (the sync's window).
pub fn in_group<'a>(saves: &'a [SaveEntry], game: &str, map: &str, kind: razdor_wire::cloud::SaveKind) -> Vec<&'a SaveEntry> {
    saves
        .iter()
        .filter(|e| e.meta.game == game && e.meta.map == map && e.meta.kind == kind)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A stub transport: answers with the status and body it was given, and records the calls.
    struct Stub {
        reply: Mutex<Result<Reply, String>>,
        calls: Mutex<Vec<(String, String, Option<String>)>>,
    }

    impl Net for Stub {
        fn send(&self, method: &str, url: &str, key: Option<&str>, _body: Option<&[u8]>) -> Result<Reply, String> {
            self.calls.lock().expect("lock").push((method.into(), url.into(), key.map(String::from)));
            self.reply.lock().expect("lock").clone()
        }
    }

    fn stub(status: u16, body: &str) -> Arc<Stub> {
        Arc::new(Stub {
            reply: Mutex::new(Ok(Reply { status, body: body.as_bytes().to_vec() })),
            calls: Mutex::new(Vec::new()),
        })
    }

    #[test]
    fn statuses_map_onto_the_error_kinds() {
        let cases = [
            (409, "{\"error\":\"full map\"}", CloudError::Conflict("full map".into())),
            (413, "{}", CloudError::Full),
            (404, "", CloudError::NotFound),
            (401, "", CloudError::Unauthorized),
            (429, "", CloudError::Offline("HTTP 429".into())),
            (503, "", CloudError::Offline("HTTP 503".into())),
            (400, "{\"error\":\"bad\"}", CloudError::Refused("bad".into())),
        ];
        for (status, body, want) in cases {
            let net = stub(status, body);
            let api = Api::new(net, "http://x", Some("k".repeat(32)));
            assert_eq!(api.list_saves().expect_err("refused"), want, "status {status}");
        }
    }

    #[test]
    fn a_keyed_call_without_a_key_is_refused_before_it_is_sent() {
        let net = stub(200, "{}");
        let api = Api::new(net.clone(), "http://x", None);
        assert_eq!(api.list_saves(), Err(CloudError::Unauthorized));
        assert!(net.calls.lock().expect("lock").is_empty(), "nothing is sent");
    }

    #[test]
    fn the_key_header_and_the_path_are_sent() {
        let net = stub(200, "{\"saves\":[],\"used_bytes\":0,\"quota_bytes\":20971520}");
        let api = Api::new(net.clone(), "http://x/", Some("ab".repeat(32)));
        let list = api.list_saves().expect("lists");
        assert!(list.saves.is_empty());
        let calls = net.calls.lock().expect("lock");
        assert_eq!(calls[0].1, "http://x/v1/saves");
        assert_eq!(calls[0].2.as_deref(), Some("ab".repeat(32).as_str()));
    }

    #[test]
    fn an_unreadable_answer_is_bad_not_a_panic() {
        let api = Api::new(stub(200, "not json"), "http://x", Some("ab".repeat(32)));
        assert!(matches!(api.list_saves(), Err(CloudError::Bad(_))));
    }

    #[test]
    fn a_transport_failure_is_offline() {
        let net = Arc::new(Stub {
            reply: Mutex::new(Err("curl: exit 7".into())),
            calls: Mutex::new(Vec::new()),
        });
        let api = Api::new(net, "http://x", Some("ab".repeat(32)));
        assert_eq!(api.list_saves(), Err(CloudError::Offline("curl: exit 7".into())));
    }
}
