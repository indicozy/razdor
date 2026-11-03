//! The studio's MCP listener: 127.0.0.1 on a random port, with a random 128-bit token. A client sends
//! `{"auth": "<token>"}` as its first line; the server answers `{"auth": "ok"}` (then JSON-RPC lines
//! follow, one reply per line), `denied` for a wrong token, or `busy` while another client is served.
//! The runtime file `studio-mcp.json` (in the app's folder, never in a package) tells the stdio bridge
//! where to connect: `{"port", "token", "pid"}`. It is written with the studio and removed with it.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::host::Host;
use super::rpc;

/// The runtime file's name, in Razdor's own folder.
pub const RUNTIME_FILE: &str = "studio-mcp.json";

/// Where the runtime file is: `studio-mcp.json` in Razdor's folder (`platform::app_dir`).
pub fn runtime_file() -> Option<PathBuf> {
    crate::platform::app_dir().map(|d| d.join(RUNTIME_FILE))
}

/// The runtime file's content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// A running listener. Dropping it stops the server and removes its runtime file.
pub struct Server {
    file: PathBuf,
    runtime: Runtime,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    /// Starts the listener on a random loopback port, and writes the runtime file to `file`.
    pub fn start(host: Arc<dyn Host>, file: PathBuf) -> Result<Server, String> {
        let token = new_token()?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
        let port = listener.local_addr().map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?.port();
        listener.set_nonblocking(true).map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
        let runtime = Runtime { port, token: token.clone(), pid: std::process::id() };
        write_runtime(&file, &runtime)?;
        let stop = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let accept = {
            let stop = stop.clone();
            thread::Builder::new()
                .name("studio-mcp".into())
                .spawn(move || accept_loop(listener, host, token, busy, stop))
                .map_err(|e| format!("cannot start the MCP listener: {e}"))?
        };
        Ok(Server { file, runtime, stop, accept: Some(accept) })
    }

    /// The port the listener is on.
    pub fn port(&self) -> u16 {
        self.runtime.port
    }

    /// The runtime file the server wrote.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
        // Only the file this server wrote: another studio may have written its own since.
        if read_runtime(&self.file).is_some_and(|r| r == self.runtime) {
            let _ = fs::remove_file(&self.file);
        }
    }
}

/// A random 128-bit token, as 32 lowercase hex digits.
fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("cannot make a token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes the runtime file, readable by its owner only (it holds the token).
fn write_runtime(file: &Path, runtime: &Runtime) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(runtime).map_err(|e| e.to_string())?;
    let tmp = file.with_extension("json.tmp");
    let _ = fs::remove_file(&tmp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    out.write_all(text.as_bytes()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    drop(out);
    fs::rename(&tmp, file).map_err(|e| format!("cannot write {}: {e}", file.display()))
}

/// The runtime file's content, if it is there and reads.
pub fn read_runtime(file: &Path) -> Option<Runtime> {
    serde_json::from_str(&fs::read_to_string(file).ok()?).ok()
}

fn accept_loop(listener: TcpListener, host: Arc<dyn Host>, token: String, busy: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let (host, token, busy) = (host.clone(), token.clone(), busy.clone());
                let _ = thread::Builder::new().name("studio-mcp-client".into()).spawn(move || serve(stream, host.as_ref(), &token, &busy));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Serves one client: the token, then its JSON-RPC lines until it closes.
fn serve(stream: TcpStream, host: &dyn Host, token: &str, busy: &AtomicBool) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut writer = stream;
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let presented = serde_json::from_str::<Value>(line.trim()).ok().and_then(|v| v.get("auth").and_then(Value::as_str).map(str::to_string));
    if !presented.as_deref().is_some_and(|p| same_token(p, token)) {
        let _ = send(&mut writer, &json!({ "auth": "denied", "error": "the token is wrong" }));
        return;
    }
    if busy.swap(true, Ordering::SeqCst) {
        let _ = send(&mut writer, &json!({ "auth": "busy", "error": "Razdor Studio already has an MCP client connected" }));
        return;
    }
    let _guard = BusyGuard(busy);
    if send(&mut writer, &json!({ "auth": "ok" })).is_err() {
        return;
    }
    let _ = writer.set_read_timeout(None);
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = line.trim_end();
        if text.is_empty() {
            continue;
        }
        if let Some(reply) = rpc::handle_line(host, text) {
            if writer.write_all(format!("{reply}\n").as_bytes()).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    }
}

/// Frees the client slot when the client's session ends.
struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn send(writer: &mut TcpStream, value: &Value) -> std::io::Result<()> {
    writer.write_all(format!("{value}\n").as_bytes())?;
    writer.flush()
}

/// Compares two tokens without stopping at the first different byte.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_128_random_bits_in_hex() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
        assert!(same_token(&a, &a));
        assert!(!same_token(&a, &b));
        assert!(!same_token("short", &a));
    }
}
