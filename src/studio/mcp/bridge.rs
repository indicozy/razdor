//! `razdor --studio-mcp`: the stdio bridge an agent starts. It reads the studio's runtime file, connects
//! to the running studio on loopback, sends the token first, and relays newline-delimited JSON-RPC both
//! ways. With no studio running (no runtime file, a refused connection, a stale file) it answers every
//! request with a JSON-RPC error saying so, and it does not open a window.

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use super::server::{read_runtime, runtime_file};

/// The error every request gets when no studio answers.
pub const NOT_RUNNING: &str = "Razdor Studio is not running";

/// `--studio-mcp` on the command line: the bridge, with its exit status. `None` when the flag is not
/// there (the program goes on as usual).
pub fn cli(args: &[String]) -> Option<i32> {
    if !args.iter().any(|a| a == "--studio-mcp") {
        return None;
    }
    Some(run(runtime_file().as_deref()))
}

/// Runs the bridge on stdin and stdout against the studio named by `file`.
pub fn run(file: Option<&Path>) -> i32 {
    match connect(file) {
        Ok(stream) => relay(stream),
        Err(reason) => {
            let stdin = std::io::stdin();
            let mut stdout = std::io::stdout();
            for line in stdin.lock().lines().map_while(Result::ok) {
                if let Some(reply) = not_running_reply(&line, &reason) {
                    let _ = writeln!(stdout, "{reply}");
                    let _ = stdout.flush();
                }
            }
            0
        }
    }
}

/// The reply to a request while no studio answers: a JSON-RPC error with `reason`. `None` for a
/// notification (no `id`), which gets no reply; a line that is not JSON gets an error with no id.
pub fn not_running_reply(line: &str, reason: &str) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }
    let id = match serde_json::from_str::<Value>(line) {
        Ok(v) => match v.get("id") {
            Some(id) => id.clone(),
            None => return None,
        },
        Err(_) => Value::Null,
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": reason } }).to_string())
}

/// Connects to the studio named by the runtime file and passes the token. The reason when it does not.
fn connect(file: Option<&Path>) -> Result<TcpStream, String> {
    let file = file.ok_or(NOT_RUNNING)?;
    let runtime = read_runtime(file).ok_or(NOT_RUNNING)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], runtime.port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|_| NOT_RUNNING.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).map_err(|e| e.to_string())?;
    writeln!(stream, "{}", json!({ "auth": runtime.token })).map_err(|_| NOT_RUNNING.to_string())?;
    let mut line = String::new();
    BufReader::new(stream.try_clone().map_err(|e| e.to_string())?).read_line(&mut line).map_err(|_| NOT_RUNNING.to_string())?;
    let answer: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
    match answer.get("auth").and_then(Value::as_str) {
        Some("ok") => {
            stream.set_read_timeout(None).map_err(|e| e.to_string())?;
            Ok(stream)
        }
        _ => Err(format!("{NOT_RUNNING}: {}", answer.get("error").and_then(Value::as_str).unwrap_or("the connection was refused"))),
    }
}

/// The relay: the agent's lines to the studio, the studio's to the agent. Ends when the studio
/// closes its side; the agent closing its input ends the request side.
fn relay(stream: TcpStream) -> i32 {
    let Ok(mut to_studio) = stream.try_clone() else { return 1 };
    let input = std::thread::Builder::new().name("studio-mcp-stdin".into()).spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if writeln!(to_studio, "{line}").and_then(|_| to_studio.flush()).is_err() {
                break;
            }
        }
        let _ = to_studio.shutdown(Shutdown::Write);
    });
    let mut stdout = std::io::stdout();
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        if writeln!(stdout, "{line}").and_then(|_| stdout.flush()).is_err() {
            break;
        }
    }
    drop(input);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_gets_the_not_running_error_and_a_notification_nothing() {
        let reply: Value = serde_json::from_str(&not_running_reply(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#, NOT_RUNNING).unwrap()).unwrap();
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["error"]["message"], NOT_RUNNING);
        assert_eq!(not_running_reply(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, NOT_RUNNING), None);
        assert_eq!(not_running_reply("", NOT_RUNNING), None);
        let broken: Value = serde_json::from_str(&not_running_reply("{oops", NOT_RUNNING).unwrap()).unwrap();
        assert_eq!(broken["id"], Value::Null);
    }

    #[test]
    fn with_no_runtime_file_the_bridge_says_not_running() {
        let missing = std::env::temp_dir().join(format!("razdor-no-studio-{}.json", std::process::id()));
        assert_eq!(connect(Some(&missing)).err().as_deref(), Some(NOT_RUNNING));
        assert_eq!(connect(None).err().as_deref(), Some(NOT_RUNNING));
    }

    #[test]
    fn the_flag_selects_the_bridge() {
        assert_eq!(cli(&["--pack".to_string(), "x".to_string()]), None);
    }
}
