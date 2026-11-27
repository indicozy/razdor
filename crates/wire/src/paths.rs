//! The API's paths and headers. The server mounts them under one host (`api.<domain>`); the game
//! takes the base address from remote config. Ids in a path are server-issued and checked by the
//! server; `save` builds the path of one save.

/// Analytics (`2026-11-22-analytics-design.md`).
pub const PING: &str = "/v1/ping";
pub const EVENTS: &str = "/v1/events";
pub const PERF: &str = "/v1/perf";
pub const CRASH: &str = "/v1/crash";
pub const CRASH_SAVE: &str = "/v1/crash/save";
pub const BATTLES: &str = "/v1/battles";
/// The verifier's verdicts: admin only ([`ADMIN_HEADER`]).
pub const VERDICTS: &str = "/v1/verdicts";
/// «Удалить мои данные»: the server deletes everything of an install id.
pub const INSTALL_DELETE: &str = "/v1/install/delete";

/// Cloud saves (`2026-11-22-cloud-saves-design.md`). `POST` creates the account, `DELETE` removes it.
pub const ACCOUNT: &str = "/v1/account";
/// `POST` issues a link code (keyed); `POST` on [`LINK_REDEEM`] gives the key to a second device.
pub const LINK: &str = "/v1/link";
pub const LINK_REDEEM: &str = "/v1/link/redeem";
/// `GET` lists the account's saves; `PUT` stores one (framed: [`crate::cloud::SaveMeta`] then the bytes).
pub const SAVES: &str = "/v1/saves";
pub const MARKS: &str = "/v1/marks";
pub const SETTINGS: &str = "/v1/settings";
pub const HUB: &str = "/v1/hub";
pub const BENCH: &str = "/v1/bench";
/// The site's counter: aggregate play stats of the last 7 days, no key.
pub const PUBLIC_STATS: &str = "/v1/public/stats";

/// Health of the service (no body, no key).
pub const HEALTH: &str = "/healthz";

/// One save's path: `GET` returns it (framed), `DELETE` removes it.
pub fn save(id: &str) -> String {
    format!("{SAVES}/{id}")
}

/// The account key of a keyed request.
pub const KEY_HEADER: &str = "x-razdor-key";
/// The app version (`CARGO_PKG_VERSION`, e.g. `1.5.4`) of every request to the API, so the server
/// and nginx can tell the builds apart.
pub const VERSION_HEADER: &str = "x-razdor-version";
/// The commit the app was built from (its short hash, with `-dirty` on an edited tree; `unknown`
/// when the build had no git), sent with every request to the API like [`VERSION_HEADER`].
pub const COMMIT_HEADER: &str = "x-razdor-commit";
/// The admin token of the verifier and the dashboard's API.
pub const ADMIN_HEADER: &str = "x-razdor-admin";
