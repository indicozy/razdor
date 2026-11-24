//! The mirror's network, behind a trait so the sync logic is tested without a network.

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

/// A whole small reply (the config, the API lists, a redirect).
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    /// Header names lower-cased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

pub trait Net {
    /// A GET that follows redirects, with the whole body. `headers` are extra request headers.
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Reply, String>;
    /// A GET that does not follow redirects (the body is not read): the catalog's `latest` tag.
    fn redirect(&self, url: &str) -> Result<Reply, String>;
    /// Appends the file from `offset` to `dest` (a Range request when `offset > 0`). A server that
    /// ignores the Range and sends the whole file starts `dest` again. Returns the HTTP status.
    fn download(&self, url: &str, offset: u64, dest: &Path) -> Result<u16, String>;
}

/// The real network: rustls through ureq. A stalled connection gives up after 30 s with no data.
pub struct Http {
    follow: ureq::Agent,
    no_redirect: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Http {
        let base = || {
            ureq::AgentBuilder::new()
                .user_agent("razdor-mirror")
                .timeout_connect(Duration::from_secs(30))
                .timeout_read(Duration::from_secs(30))
        };
        Http { follow: base().build(), no_redirect: base().redirects(0).build() }
    }

    fn collect(resp: ureq::Response, read_body: bool) -> Result<Reply, String> {
        let status = resp.status();
        let names = resp.headers_names();
        let headers = names
            .iter()
            .filter_map(|n| resp.header(n).map(|v| (n.to_ascii_lowercase(), v.to_string())))
            .collect();
        let mut body = Vec::new();
        if read_body {
            std::io::Read::read_to_end(&mut resp.into_reader(), &mut body).map_err(|e| e.to_string())?;
        }
        Ok(Reply { status, headers, body })
    }

    fn reply(result: Result<ureq::Response, ureq::Error>, read_body: bool) -> Result<Reply, String> {
        match result {
            Ok(resp) => Self::collect(resp, read_body),
            // A 3xx without redirects, and 4xx/5xx, come back as status errors that still carry the reply.
            Err(ureq::Error::Status(_, resp)) => Self::collect(resp, read_body),
            Err(e) => Err(e.to_string()),
        }
    }
}

impl Net for Http {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Reply, String> {
        let mut req = self.follow.get(url);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        Self::reply(req.call(), true)
    }

    fn redirect(&self, url: &str) -> Result<Reply, String> {
        Self::reply(self.no_redirect.get(url).call(), false)
    }

    fn download(&self, url: &str, offset: u64, dest: &Path) -> Result<u16, String> {
        let mut req = self.follow.get(url);
        if offset > 0 {
            req = req.set("Range", &format!("bytes={offset}-"));
        }
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return Err(format!("HTTP {code}")),
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let mut file = if status == 206 {
            OpenOptions::new().create(true).append(true).open(dest).map_err(|e| e.to_string())?
        } else if status == 200 {
            OpenOptions::new().create(true).write(true).truncate(true).open(dest).map_err(|e| e.to_string())?
        } else {
            return Err(format!("HTTP {status}"));
        };
        file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        std::io::copy(&mut resp.into_reader(), &mut file).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        Ok(status)
    }
}
