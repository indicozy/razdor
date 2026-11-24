//! The download mirror: `razdor-mirror` pulls the owner's public release files from GitHub into the web root
//! that nginx serves at `/hub/`. Nothing is visible until its release folder is complete (one rename); the
//! feed and `latest` move last. Spec: `docs/specs/own-mirror-2026-11-24.md`, section 3.1.

pub mod config;
pub mod github;
pub mod net;
pub mod site;
pub mod store;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use config::MirrorConfig;
use github::{Asset, Release, OWNER};
use net::{Net, Reply};

pub const CONFIG_URL: &str = "https://indicozy.github.io/razdor/config.json";
pub const APP_API: &str = "https://api.github.com/repos/indicozy/razdor/releases?per_page=30";
pub const HUB_API: &str = "https://api.github.com/repos/indicozy/razdor-community/releases?per_page=100";
/// The owner's site copy: the pre-release `site-latest` of this repo (see `site.rs`). Read only when the app list lacks it.
pub const SITE_API: &str = "https://api.github.com/repos/indicozy/razdor/releases/tags/site-latest";
pub const CATALOG_LATEST: &str = "https://github.com/indicozy/razdor-community/releases/latest/download/index.json";
/// The depth model's release (`src/depth/fetch.rs`, `DEPTH_MODEL_URL`).
pub const DEPTH_TAG: &str = "depth-model-v1";

const APP: &str = "razdor";
const HUB: &str = "razdor-community";
/// Tries per file (each one resumes the partial file).
const TRIES: u32 = 3;
const DAY: Duration = Duration::from_secs(86_400);

/// What a run did. `errors` lists what could not be published; the run then exits 1.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// The app tags the feed lists (complete on disk).
    pub app: Vec<String>,
    /// The catalog tag `latest` points at after the run.
    pub catalog: Option<String>,
    pub errors: Vec<String>,
    /// GitHub API requests this run made (the budget is 2 a run: the app list, and the catalog list when a new
    /// catalog needs it; plus one lookup per tag a list lacks, the site release's among them).
    pub api_calls: u32,
    /// GitHub refused for the rate limit: the run ended quietly and changed nothing.
    pub rate_limited: bool,
}

/// A stop in the middle of a run: the rate limit ends it quietly, anything else is a failure.
enum Stop {
    RateLimited,
    Fatal(String),
}

impl From<String> for Stop {
    fn from(e: String) -> Stop {
        Stop::Fatal(e)
    }
}

fn io_stop<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> Stop + '_ {
    move |e| Stop::Fatal(format!("{what}: {e}"))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    App,
    Catalog,
    Extra,
}

/// One file of a release: its address and the SHA-256 it must have.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// One release folder to fetch: the wanted files and what is wrong with it.
#[derive(Debug, Clone)]
struct Job {
    repo: &'static str,
    tag: String,
    kind: Kind,
    files: Vec<Spec>,
    problems: Vec<String>,
}

/// Runs one sync. `Err` is a failure before anything was published (exit 1); a file or release that
/// failed is in `Outcome::errors`.
pub fn run(cfg: &MirrorConfig, net: &dyn Net, now: SystemTime) -> Result<Outcome, String> {
    let mut s = Sync { cfg, net, out: Outcome::default(), hub: None, missing: HashSet::new() };
    match s.sync(now) {
        Ok(()) => Ok(s.out),
        Err(Stop::RateLimited) => {
            s.out.rate_limited = true;
            Ok(s.out)
        }
        Err(Stop::Fatal(e)) => Err(e),
    }
}

struct Sync<'a> {
    cfg: &'a MirrorConfig,
    net: &'a dyn Net,
    out: Outcome,
    /// The razdor-community release list, read at most once a run.
    hub: Option<Vec<Release>>,
    /// Tags a single-release lookup found missing, so a run asks once.
    missing: HashSet<String>,
}

/// The hub's stage: `<root>/.stage`. It sits inside the web root, so every publish is a rename within the root's
/// mount (the unit's bind mounts would make a rename from another folder fail with EXDEV). nginx denies dot paths.
fn hub_stage(cfg: &MirrorConfig) -> PathBuf {
    cfg.root.join(".stage")
}

/// The site's stage: `<site_dir>/.stage`, for the same reason as `hub_stage`.
fn site_stage(cfg: &MirrorConfig) -> PathBuf {
    cfg.site_dir.join(".stage")
}

fn stage_dir(cfg: &MirrorConfig, repo: &str, tag: &str) -> PathBuf {
    hub_stage(cfg).join(repo).join(tag)
}

impl Sync<'_> {
    fn sync(&mut self, now: SystemTime) -> Result<(), Stop> {
        let cfg = self.cfg;
        fs::create_dir_all(&cfg.root).map_err(io_stop("web root"))?;
        // The lock folder: the run state and the API cache. Downloads are never staged here (see `hub_stage`).
        fs::create_dir_all(&cfg.stage).map_err(io_stop("stage"))?;
        // Before any download: each stage must publish into its folder (a real rename, not st_dev).
        for dest in [download_dir(&cfg.root, APP), download_dir(&cfg.root, HUB)] {
            store::probe_move(&hub_stage(cfg), &dest).map_err(Stop::Fatal)?;
        }
        store::remove_stale(&hub_stage(cfg), now, DAY).map_err(io_stop("stage cleanup"))?;
        store::remove_stale(&cfg.stage, now, DAY).map_err(io_stop("stage cleanup"))?;
        if let Err(e) = store::remove_stale(&site_stage(cfg), now, DAY) {
            self.out.errors.push(format!("site stage cleanup: {e}"));
        }

        // 1. The config: from GitHub Pages (no API quota). Refused unless it is a schema-1 config.
        let reply = self.net.get(CONFIG_URL, &[])?;
        if reply.status != 200 {
            return Err(Stop::Fatal(format!("config: HTTP {}", reply.status)));
        }
        github::check_config(&reply.body)?;
        let config_body = reply.body;

        // 2. The app's release list (API call 1).
        let app_list = self.api(APP_API, "app")?;
        let kept: Vec<&Release> = github::newest_published(&app_list, cfg.keep_app_releases);

        // 3. The catalog: which tag `latest` names now (a redirect, no API call).
        let latest = self.catalog_tag();
        let mut jobs: Vec<Job> = Vec::new();
        for rel in &kept {
            if !store::tag_dir(&cfg.root, APP, &rel.tag).exists() {
                jobs.push(self.build_job(APP, rel, Kind::App)?);
            }
        }
        let mut catalog_job: Option<Job> = None;
        if let Some(tag) = &latest {
            if !store::tag_dir(&cfg.root, HUB, tag).exists() {
                match self.hub_release(tag)? {
                    Some(rel) => catalog_job = Some(self.build_job(HUB, &rel, Kind::Catalog)?),
                    None => self.out.errors.push(format!("catalog {tag}: no such release")),
                }
            }
        }

        // 4. The catalog's index names the game and the ad (their releases are fetched too); the depth
        //    model is always wanted. index.json is fetched first, on its own, so the list is known.
        let mut index: Option<Vec<u8>> = None;
        if let Some(job) = catalog_job.as_mut() {
            match job.files.iter().find(|f| f.name == "index.json").cloned() {
                None => job.problems.push("no index.json".into()),
                Some(spec) => {
                    let dir = stage_dir(cfg, HUB, &job.tag);
                    fs::create_dir_all(&dir).map_err(io_stop("stage"))?;
                    match self.fetch_file(&spec, &dir) {
                        Ok(()) => index = fs::read(dir.join("index.json")).ok(),
                        Err(e) => job.problems.push(format!("index.json: {e}")),
                    }
                }
            }
        } else if let Some(tag) = &latest {
            index = fs::read(store::tag_dir(&cfg.root, HUB, tag).join("index.json")).ok();
        }
        let mut wanted_extras: Vec<String> = Vec::new();
        if let Some(bytes) = &index {
            for (repo, tag) in github::referenced_tags(bytes, &cfg.own_base) {
                if repo == HUB {
                    wanted_extras.push(tag);
                }
            }
        }
        wanted_extras.push(DEPTH_TAG.to_string());
        let mut extra_jobs: Vec<Job> = Vec::new();
        for tag in wanted_extras {
            // The catalog's own release (the ad image lives in it) is the catalog job, not an extra.
            let is_catalog = latest.as_deref() == Some(tag.as_str());
            if is_catalog || store::tag_dir(&cfg.root, HUB, &tag).exists() || extra_jobs.iter().any(|j| j.tag == tag) {
                continue;
            }
            match self.hub_release(&tag)? {
                Some(rel) => extra_jobs.push(self.build_job(HUB, &rel, Kind::Extra)?),
                None => {
                    // A catalog that names it cannot publish; the depth model alone is an error to log.
                    if let Some(job) = catalog_job.as_mut() {
                        job.problems.push(format!("needs {tag}, which is not a release"));
                    } else {
                        self.out.errors.push(format!("{tag}: no such release"));
                    }
                }
            }
        }
        jobs.extend(extra_jobs.iter().cloned());
        if let Some(job) = catalog_job.clone() {
            jobs.push(job);
        }

        // 5. Disk: the planned new bytes must fit under the cap and leave the free floor. Nothing is
        //    downloaded when they do not.
        let planned: u64 = jobs
            .iter()
            .flat_map(|j| {
                let mut seen = HashSet::new();
                j.files
                    .iter()
                    .filter(move |f| !store::tag_dir(&cfg.root, j.repo, &j.tag).join(&f.name).exists() && seen.insert(f.sha256.clone()))
                    .map(|f| f.size)
            })
            .sum();
        let used = store::used_bytes(&cfg.root).map_err(io_stop("web root size"))?;
        let free = store::free_bytes(&cfg.root).map_err(io_stop("free space"))?;
        if used + planned > cfg.max_bytes || free.saturating_sub(planned) < cfg.min_free_bytes {
            return Err(Stop::Fatal(format!(
                "disk cap: {planned} new bytes would pass max_bytes or min_free_bytes; nothing downloaded"
            )));
        }

        // 6. Fetch every job's files into the stage, then publish what is complete.
        for job in jobs.iter_mut() {
            self.fetch_job(job)?;
        }
        // The app and extra releases first: the catalog names them.
        for job in jobs.iter().filter(|j| j.kind != Kind::Catalog) {
            for p in &job.problems {
                self.out.errors.push(format!("{} {}: {p}", job.repo, job.tag));
            }
            if job.problems.is_empty() {
                store::publish_dir(&stage_dir(cfg, job.repo, &job.tag), &store::tag_dir(&cfg.root, job.repo, &job.tag))
                    .map_err(io_stop("publish"))?;
            }
        }
        // The catalog last, and only when every file it names is on disk. Its game address becomes the
        // own copy (option B) before it is published.
        let mut catalog_published: Option<String> = None;
        if let Some(job) = jobs.iter().find(|j| j.kind == Kind::Catalog).cloned() {
            let mut problems = job.problems.clone();
            let stage = stage_dir(cfg, HUB, &job.tag);
            let refs = index.as_deref().map(|b| github::referenced_tags(b, &cfg.own_base)).unwrap_or_default();
            for (repo, tag) in refs {
                // The catalog's own files (its ad image) are part of the job itself.
                if repo == HUB && tag != job.tag && !store::tag_dir(&cfg.root, HUB, &tag).exists() {
                    problems.push(format!("{tag} is not on disk"));
                }
            }
            if problems.is_empty() {
                let bytes = fs::read(stage.join("index.json")).map_err(io_stop("catalog"))?;
                match github::rewrite_catalog(&bytes, &cfg.own_base) {
                    Ok(new) => store::write_atomic(&stage.join("index.json"), &new).map_err(io_stop("catalog"))?,
                    Err(e) => problems.push(e),
                }
            }
            if problems.is_empty() {
                store::publish_dir(&stage, &store::tag_dir(&cfg.root, HUB, &job.tag)).map_err(io_stop("publish"))?;
                catalog_published = Some(job.tag.clone());
            } else {
                for p in &problems {
                    self.out.errors.push(format!("catalog {}: {p}", job.tag));
                }
            }
        }

        // 6b. The owner's site copy (`site.rs`): fetched, unpacked and swapped in, or left as it is.
        let site = self.sync_site(&app_list, &read_status(&cfg.root))?;

        // 7. Then `latest` (the catalog's files are on disk), the feed, the config, and status.json.
        if let Some(tag) = latest.as_ref() {
            if store::tag_dir(&cfg.root, HUB, tag).exists() {
                store::replace_symlink(&store::latest_link(&cfg.root, HUB), &format!("../download/{tag}"))
                    .map_err(io_stop("latest"))?;
            }
        }
        let complete: Vec<&Release> = kept.iter().copied().filter(|r| store::tag_dir(&cfg.root, APP, &r.tag).exists()).collect();
        let feed = github::feed(&complete, |r| github::wanted(r, cfg).into_iter().cloned().collect());
        let feed_bytes = serde_json::to_vec_pretty(&feed).map_err(|e| Stop::Fatal(format!("feed: {e}")))?;
        store::write_atomic(&cfg.root.join("pages").join(HUB).join("releases.json"), &feed_bytes)
            .map_err(io_stop("feed"))?;
        store::write_atomic(&cfg.root.join("pages").join("razdor").join("config.json"), &config_body)
            .map_err(io_stop("config"))?;
        self.out.app = complete.iter().map(|r| r.tag.clone()).collect();

        let previous = read_status(&cfg.root);
        let mut history = previous.catalog_history.clone();
        if let Some(tag) = &catalog_published {
            history.retain(|t| t != tag);
            history.insert(0, tag.clone());
        }
        history.truncate(cfg.keep_catalogs);
        self.out.catalog = match (&catalog_published, &latest) {
            (Some(tag), _) => Some(tag.clone()),
            (None, Some(tag)) if store::tag_dir(&cfg.root, HUB, tag).exists() => Some(tag.clone()),
            _ => previous.catalog.clone(),
        };
        let status = json!({
            "synced_at": rfc3339(now),
            "app": self.out.app,
            "catalog": self.out.catalog,
            "catalog_history": history,
            "config_sha256": store::sha256_bytes(&config_body),
            "site": site.live,
            "site_previous": site.previous,
            "errors": self.out.errors,
            "ok": self.out.errors.is_empty(),
        });
        store::write_atomic(&cfg.root.join("status.json"), &serde_json::to_vec_pretty(&status).unwrap_or_default())
            .map_err(io_stop("status"))?;

        // 8. Prune: app releases not kept (or withdrawn: they left the list), catalogs beyond the history,
        //    the game and ad releases no kept catalog names (the depth model stays).
        self.prune(&complete, &history)?;
        Ok(())
    }

    /// The owner's site copy. The release `site-latest` holds `site.tar.gz`; when its digest is not the one
    /// `site_dir/current` names, the archive is fetched (verified), unpacked into a new folder under
    /// `versions/`, and `current` is moved to it with one rename. Nothing is removed before that. A failure
    /// leaves the copy in place and is an error of the run. No release yet is not an error (the first deploy).
    fn sync_site(&mut self, app_list: &[Release], before: &PreviousStatus) -> Result<SiteState, Stop> {
        let cfg = self.cfg;
        let versions = cfg.site_dir.join("versions");
        let current = cfg.site_dir.join("current");
        let live_now = fs::read_link(&current)
            .ok()
            .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()));
        let mut state = SiteState { live: live_now.clone(), previous: before.site_previous.clone() };
        // A failure here is an error of the run, never a reason to stop it: the hub, the feed and the status
        // are written after this step.
        if let Err(e) = fs::create_dir_all(&versions) {
            self.out.errors.push(format!("site: {}: {e}", versions.display()));
            return Ok(state);
        }
        // The app list usually holds the site release (the newest pre-release); a tag lookup only when it does not.
        let listed = app_list.iter().find(|r| r.tag == site::SITE_TAG && !r.draft).cloned();
        let rel = match listed {
            Some(r) => r,
            None => match self.api(SITE_API, "site") {
                Ok(found) => match found.into_iter().find(|r| r.tag == site::SITE_TAG && !r.draft) {
                    Some(r) => r,
                    None => return Ok(state),
                },
                Err(Stop::RateLimited) => return Err(Stop::RateLimited),
                Err(Stop::Fatal(e)) => {
                    self.out.errors.push(format!("site: {e}"));
                    return Ok(state);
                }
            },
        };
        let Some(asset) = rel.assets.iter().find(|a| a.name == site::SITE_ASSET).cloned() else {
            self.out.errors.push(format!("site: {} has no {}", site::SITE_TAG, site::SITE_ASSET));
            return Ok(state);
        };
        let Some(sha) = asset.digest.clone() else {
            self.out.errors.push("site: the archive has no SHA-256 digest".into());
            return Ok(state);
        };
        if live_now.as_deref() != Some(sha.as_str()) {
            if !versions.join(&sha).is_dir() {
                if let Err(e) = self.build_site_version(&asset, &sha) {
                    self.out.errors.push(format!("site: {e}"));
                    return Ok(state);
                }
            }
            if let Err(e) = store::replace_symlink(&current, &format!("versions/{sha}")) {
                self.out.errors.push(format!("site: current: {e}"));
                return Ok(state);
            }
            state.previous = live_now.filter(|l| *l != sha);
        }
        state.live = Some(sha.clone());

        // Keep the live copy and the one before it (a rollback); the rest goes.
        let mut keep: HashSet<String> = HashSet::new();
        keep.extend(state.live.clone());
        keep.extend(state.previous.clone());
        remove_unlisted(&versions, &keep, &mut self.out.errors);
        if state.live.as_deref() == Some(sha.as_str()) {
            let _ = fs::remove_dir_all(site_stage(cfg).join("site"));
        }
        Ok(state)
    }

    /// Fetches the archive into the stage, checks it, unpacks it into a stage folder and moves that folder
    /// into `versions/<sha>`. Any failure leaves `versions/` as it was.
    fn build_site_version(&self, asset: &Asset, sha: &str) -> Result<(), String> {
        let cfg = self.cfg;
        let versions = cfg.site_dir.join("versions");
        // Before the download: the stage must publish into `versions` (a real rename, not st_dev).
        store::probe_move(&site_stage(cfg), &versions)?;
        let stage = site_stage(cfg).join("site");
        fs::create_dir_all(&stage).map_err(|e| format!("stage: {e}"))?;
        let spec = Spec { name: site::SITE_ASSET.into(), url: asset.url.clone(), size: asset.size, sha256: sha.into() };
        self.fetch_file(&spec, &stage)?;
        let unpacked = stage.join(format!("unpacked-{sha}"));
        let _ = fs::remove_dir_all(&unpacked);
        site::unpack(&stage.join(site::SITE_ASSET), &unpacked)?;
        site::check(&unpacked)?;
        store::rename_within_mount(&unpacked, &versions.join(sha)).map_err(|e| format!("publish: {e}"))
    }

    fn prune(&mut self, complete: &[&Release], history: &[String]) -> Result<(), Stop> {
        let cfg = self.cfg;
        // App releases: the kept, complete ones. A release that left the list is not kept either.
        let keep_app: HashSet<String> = complete.iter().map(|r| r.tag.clone()).collect();
        remove_unlisted(&download_dir(&cfg.root, APP), &keep_app, &mut self.out.errors);

        // Catalogs: the history. Other razdor-community releases: the depth model and what a kept catalog names.
        let mut keep_hub: HashSet<String> = history.iter().cloned().collect();
        keep_hub.insert(DEPTH_TAG.to_string());
        for h in history {
            if let Ok(bytes) = fs::read(store::tag_dir(&cfg.root, HUB, h).join("index.json")) {
                for (_, tag) in github::referenced_tags(&bytes, &cfg.own_base) {
                    keep_hub.insert(tag);
                }
            }
        }
        remove_unlisted(&download_dir(&cfg.root, HUB), &keep_hub, &mut self.out.errors);
        Ok(())
    }

    /// The catalog tag `latest` names: the redirect's `Location`. A failure is logged and the catalog
    /// is left for the next run.
    fn catalog_tag(&mut self) -> Option<String> {
        match self.net.redirect(CATALOG_LATEST) {
            Ok(r) if matches!(r.status, 301 | 302 | 303 | 307 | 308) => {
                let tag = r.header("location").and_then(github::tag_from_location);
                if tag.is_none() {
                    self.out.errors.push("catalog: the redirect names no tag".into());
                }
                tag
            }
            Ok(r) => {
                self.out.errors.push(format!("catalog: HTTP {} for latest", r.status));
                None
            }
            Err(e) => {
                self.out.errors.push(format!("catalog: {e}"));
                None
            }
        }
    }

    /// The razdor-community release of a tag: from the list (read once a run), else one lookup for the tag.
    fn hub_release(&mut self, tag: &str) -> Result<Option<Release>, Stop> {
        if self.hub.is_none() {
            let list = self.api(HUB_API, "hub")?;
            self.hub = Some(list);
        }
        if let Some(r) = self.hub.as_ref().and_then(|l| l.iter().find(|r| r.tag == tag && !r.draft)) {
            return Ok(Some(r.clone()));
        }
        if self.missing.contains(tag) {
            return Ok(None);
        }
        let url = format!("https://api.github.com/repos/{OWNER}/{HUB}/releases/tags/{tag}");
        let found = self.api(&url, &format!("tag-{tag}"))?;
        match found.into_iter().find(|r| r.tag == tag && !r.draft) {
            Some(r) => Ok(Some(r)),
            None => {
                self.missing.insert(tag.to_string());
                Ok(None)
            }
        }
    }

    /// A GitHub API GET (counted). Answers from the cached copy on 304; a 404 is no releases.
    fn api(&mut self, url: &str, name: &str) -> Result<Vec<Release>, Stop> {
        self.out.api_calls += 1;
        let dir = self.cfg.stage.join("api");
        fs::create_dir_all(&dir).map_err(io_stop("stage"))?;
        let cache = dir.join(format!("{name}.json"));
        let etag_file = dir.join(format!("{name}.etag"));
        let cached = fs::read(&cache).ok();
        let mut headers = Vec::new();
        if cached.is_some() {
            if let Ok(etag) = fs::read_to_string(&etag_file) {
                headers.push(("If-None-Match".to_string(), etag.trim().to_string()));
            }
        }
        let reply: Reply = self.net.get(url, &headers)?;
        let body = match reply.status {
            200 => {
                store::write_atomic(&cache, &reply.body).map_err(io_stop("api cache"))?;
                if let Some(etag) = reply.header("etag") {
                    store::write_atomic(&etag_file, etag.as_bytes()).map_err(io_stop("api cache"))?;
                }
                reply.body
            }
            304 => cached.ok_or_else(|| Stop::Fatal(format!("{name}: 304 without a cached copy")))?,
            404 => return Ok(Vec::new()),
            403 | 429 if reply.header("x-ratelimit-remaining") == Some("0") || reply.header("retry-after").is_some() => {
                return Err(Stop::RateLimited)
            }
            other => return Err(Stop::Fatal(format!("GitHub API HTTP {other} for {name}"))),
        };
        Ok(github::parse_releases(&body)?)
    }

    /// The job for a release: its wanted files with their SHA-256. For app releases the SHA256SUMS file
    /// supplies the hash of a file that has no digest.
    fn build_job(&mut self, repo: &'static str, rel: &Release, kind: Kind) -> Result<Job, Stop> {
        let mut job = Job { repo, tag: rel.tag.clone(), kind, files: Vec::new(), problems: Vec::new() };
        let wanted = github::wanted(rel, self.cfg);
        let sums: HashMap<String, String> = match (kind, wanted.iter().find(|a| a.name == "SHA256SUMS")) {
            (Kind::App, Some(a)) => self.small_file(a)?.map(|b| github::parse_sums(&b).into_iter().collect()).unwrap_or_default(),
            _ => HashMap::new(),
        };
        for a in wanted {
            match a.digest.clone().or_else(|| sums.get(&a.name).cloned()) {
                Some(sha256) => job.files.push(Spec { name: a.name.clone(), url: a.url.clone(), size: a.size, sha256 }),
                None => job.problems.push(format!("{}: no SHA-256 (not served)", a.name)),
            }
        }
        Ok(job)
    }

    /// A small file read whole (SHA256SUMS). Checked by its digest when GitHub gives one.
    fn small_file(&mut self, a: &Asset) -> Result<Option<Vec<u8>>, Stop> {
        let reply = self.net.get(&a.url, &[])?;
        if reply.status != 200 {
            return Ok(None);
        }
        if let Some(d) = &a.digest {
            if store::sha256_bytes(&reply.body) != *d {
                return Ok(None);
            }
        }
        Ok(Some(reply.body))
    }

    /// Fetches every file of a job into its stage folder; a file that fails is a problem of the job. A
    /// file with the same SHA-256 as one already fetched is a hard link to it.
    fn fetch_job(&mut self, job: &mut Job) -> Result<(), Stop> {
        let dir = stage_dir(self.cfg, job.repo, &job.tag);
        fs::create_dir_all(&dir).map_err(io_stop("stage"))?;
        let mut by_sha: HashMap<String, PathBuf> = HashMap::new();
        for spec in job.files.clone() {
            if let Some(first) = by_sha.get(&spec.sha256).cloned() {
                let dest = dir.join(&spec.name);
                if !dest.exists() {
                    store::link_or_copy(&first, &dest).map_err(io_stop("link"))?;
                }
                continue;
            }
            match self.fetch_file(&spec, &dir) {
                Ok(()) => {
                    by_sha.insert(spec.sha256.clone(), dir.join(&spec.name));
                }
                Err(e) => job.problems.push(format!("{}: {e}", spec.name)),
            }
        }
        Ok(())
    }

    /// One file into `dir`, verified: resumes a partial file with a Range request, checks the size and the
    /// SHA-256, and renames it into place only then.
    fn fetch_file(&self, spec: &Spec, dir: &Path) -> Result<(), String> {
        let dest = dir.join(&spec.name);
        if dest.exists() {
            let size_ok = spec.size == 0 || fs::metadata(&dest).map(|m| m.len() == spec.size).unwrap_or(false);
            if size_ok && store::sha256_file(&dest).map_err(|e| e.to_string())? == spec.sha256 {
                return Ok(());
            }
            fs::remove_file(&dest).map_err(|e| e.to_string())?;
        }
        let part = dir.join(format!("{}.part", spec.name));
        let mut last = String::from("no try");
        for _ in 0..TRIES {
            let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            if spec.size > 0 && have > spec.size {
                fs::remove_file(&part).map_err(|e| e.to_string())?;
                have = 0;
            }
            if spec.size == 0 || have < spec.size {
                if let Err(e) = self.net.download(&spec.url, have, &part) {
                    last = e;
                    continue;
                }
            }
            let got = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            if spec.size > 0 && got != spec.size {
                last = format!("size {got}, want {}", spec.size);
                continue;
            }
            let sha = store::sha256_file(&part).map_err(|e| e.to_string())?;
            if sha != spec.sha256 {
                fs::remove_file(&part).map_err(|e| e.to_string())?;
                return Err("SHA-256 differs from GitHub's".into());
            }
            fs::rename(&part, &dest).map_err(|e| e.to_string())?;
            return Ok(());
        }
        Err(last)
    }
}

/// The site copy after a run: the version `current` names, and the one before it (kept for a rollback).
#[derive(Debug, Default, PartialEq)]
struct SiteState {
    live: Option<String>,
    previous: Option<String>,
}

/// The folder that holds a repository's release folders.
fn download_dir(root: &Path, repo: &str) -> PathBuf {
    root.join(OWNER).join(repo).join("releases").join("download")
}

/// Removes the release folders of `dir` that are not in `keep`, reporting what fails. Symlinks stay.
fn remove_unlisted(dir: &Path, keep: &HashSet<String>, errors: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !store::is_dir_not_link(&entry.path()) || keep.contains(&name) {
            continue;
        }
        if let Err(e) = fs::remove_dir_all(entry.path()) {
            errors.push(format!("prune {name}: {e}"));
        }
    }
}

#[derive(Default)]
struct PreviousStatus {
    catalog: Option<String>,
    catalog_history: Vec<String>,
    site_previous: Option<String>,
}

fn read_status(root: &Path) -> PreviousStatus {
    let Ok(bytes) = fs::read(root.join("status.json")) else {
        return PreviousStatus::default();
    };
    let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
        return PreviousStatus::default();
    };
    PreviousStatus {
        site_previous: v.get("site_previous").and_then(Value::as_str).map(str::to_string),
        catalog: v.get("catalog").and_then(Value::as_str).map(str::to_string),
        catalog_history: v
            .get("catalog_history")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
    }
}

/// `secs` since the epoch as an RFC 3339 UTC string (`2026-11-24T16:00:00Z`).
pub fn rfc3339(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from the day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
