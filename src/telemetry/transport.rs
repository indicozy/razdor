//! How the sender posts a request. The live one goes through the app's own HTTP (`update::post`:
//! curl on the desktop, the app's Java connection on Android); the tests inject their own.

/// The server's answer: the status and the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Posts `body` (JSON, or a frame) to `url`. `Err` is a transport failure (no answer at all).
pub trait Transport: Send + Sync {
    fn post(&self, url: &str, body: &[u8], seconds: u32) -> Result<Reply, String>;
}

/// The app's HTTP.
pub struct Live;

impl Transport for Live {
    fn post(&self, url: &str, body: &[u8], seconds: u32) -> Result<Reply, String> {
        crate::update::post(url, body, seconds).map(|(status, body)| Reply { status, body })
    }
}
