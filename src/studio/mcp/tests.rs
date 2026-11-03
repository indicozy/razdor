//! The MCP server in-process: a server with a fake window and a client over loopback TCP, the way the
//! stdio bridge talks to it. The window's tools are faked; `check` runs on the real hello-story package.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::host::{FocusTarget, Frame, Host, SnapTarget};
use super::server::{read_runtime, Server};
use crate::studio::build::Job;
use crate::studio::map::Selection;

/// A window with a fixed package (the hello-story example) and canned answers.
struct FakeWindow {
    root: PathBuf,
}

impl Host for FakeWindow {
    fn job(&self) -> Result<Job, String> {
        Ok(Job { root: self.root.clone(), overlays: Vec::new(), map: Some("first-light".into()), lang: "en".into(), install: None })
    }

    fn state(&self) -> Result<Value, String> {
        Ok(json!({ "package": self.root.display().to_string(), "map": "first-light", "selection": null, "dirty": [], "stale": null }))
    }

    fn focus(&self, target: FocusTarget) -> Result<Value, String> {
        match target {
            FocusTarget::Object { sel: Selection::Hero, .. } => Ok(json!({ "ok": true, "selected": "hero" })),
            other => Err(format!("the fake window does not focus {other:?}")),
        }
    }

    fn snapshot(&self, target: SnapTarget) -> Result<Frame, String> {
        match target {
            SnapTarget::Canvas => Ok(Frame { width: 2, height: 2, rgba: vec![255; 16] }),
            SnapTarget::Window => Err("no window in the test".into()),
        }
    }
}

/// A runtime file of the test's own, removed by the test.
fn runtime_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("razdor-mcp-{name}-{}.json", std::process::id()))
}

fn hello_story() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story")
}

/// A client connection: the token sent, the answer read.
struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Client {
    fn open(port: u16, token: &str) -> (Client, Value) {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("the server accepts");
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut client = Client { stream, reader };
        writeln!(client.stream, "{}", json!({ "auth": token })).unwrap();
        let answer = client.read_value();
        (client, answer)
    }

    fn read_value(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("a line");
        serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("not JSON: {line:?}: {e}"))
    }

    fn call(&mut self, msg: Value) -> Value {
        writeln!(self.stream, "{msg}").unwrap();
        self.read_value()
    }
}

fn start(name: &str) -> (Server, PathBuf) {
    let file = runtime_path(name);
    let host: Arc<dyn Host> = Arc::new(FakeWindow { root: hello_story() });
    let server = Server::start(host, file.clone()).expect("the server starts");
    (server, file)
}

#[test]
fn a_client_with_the_token_talks_json_rpc_to_the_studio() {
    let (server, file) = start("talk");
    let runtime = read_runtime(&file).expect("the runtime file is written");
    assert_eq!(runtime.port, server.port());
    assert_eq!(runtime.pid, std::process::id());

    let (mut client, auth) = Client::open(runtime.port, &runtime.token);
    assert_eq!(auth, json!({ "auth": "ok" }));

    let init = client.call(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    writeln!(client.stream, "{}", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).unwrap();

    let listed = client.call(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
    let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["get_state", "check", "sim", "query_refs", "focus", "snapshot"]);

    let state = client.call(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "get_state", "arguments": {} } }));
    assert!(state["result"]["content"][0]["text"].as_str().unwrap().contains("first-light"));

    drop(client);
    drop(server);
    assert!(!file.exists(), "the runtime file goes with the studio");
}

#[test]
fn a_wrong_token_is_refused_and_a_second_client_is_told_the_studio_is_busy() {
    let (server, file) = start("token");
    let runtime = read_runtime(&file).unwrap();
    let (_refused, answer) = Client::open(runtime.port, "not-the-token");
    assert_eq!(answer["auth"], "denied");

    let (_first, ok) = Client::open(runtime.port, &runtime.token);
    assert_eq!(ok["auth"], "ok");
    let (_second, busy) = Client::open(runtime.port, &runtime.token);
    assert_eq!(busy["auth"], "busy");
    assert!(busy["error"].as_str().unwrap().contains("already has an MCP client"));
    drop(server);
}

#[test]
fn check_on_hello_story_through_the_server_is_the_cli_report() {
    let (server, file) = start("check");
    let runtime = read_runtime(&file).unwrap();
    let (mut client, _) = Client::open(runtime.port, &runtime.token);
    let reply = client.call(json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "check", "arguments": {} } }));
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let report: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    // The same shape as `razdor-pack check --json`: ok and the diagnostics, each with its fields.
    assert!(report["ok"].is_boolean());
    assert!(report["diagnostics"].is_array());
    for d in report["diagnostics"].as_array().unwrap() {
        for key in ["severity", "file", "line", "message", "hint"] {
            assert!(d.get(key).is_some(), "{key} in {d}");
        }
    }
    drop(server);
}

#[test]
fn the_window_tools_report_their_answers_and_their_failures() {
    let (server, file) = start("window");
    let runtime = read_runtime(&file).unwrap();
    let (mut client, _) = Client::open(runtime.port, &runtime.token);
    let focus = client.call(json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "focus", "arguments": { "kind": "hero" } } }));
    assert_eq!(focus["result"]["isError"], false, "{focus}");
    let bad = client.call(json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "focus", "arguments": { "kind": "army" } } }));
    assert_eq!(bad["result"]["isError"], true);
    assert!(bad["result"]["content"][0]["text"].as_str().unwrap().contains("`id` is required"));
    let days = client.call(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "sim", "arguments": { "days": 400 } } }));
    assert_eq!(days["result"]["isError"], true);
    assert!(days["result"]["content"][0]["text"].as_str().unwrap().contains("between 1 and 365"));
    let shot = client.call(json!({ "jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": { "name": "snapshot", "arguments": {} } }));
    assert_eq!(shot["result"]["content"][1]["type"], "image");
    assert_eq!(shot["result"]["content"][1]["mimeType"], "image/png");
    let data = shot["result"]["content"][1]["data"].as_str().unwrap();
    assert!(data.starts_with("iVBORw0KGgo"), "a PNG in base64");
    drop(server);
}

#[test]
fn query_refs_finds_the_variable_and_the_army_uses_of_hello_story() {
    let (server, file) = start("refs");
    let runtime = read_runtime(&file).unwrap();
    let (mut client, _) = Client::open(runtime.port, &runtime.token);
    let reply = client.call(json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": "query_refs", "arguments": { "kind": "variable", "id": "no_such_var" } } }));
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let found: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(found["uses"], json!([]));
    assert!(found["covers"].as_str().unwrap().contains("dialog choices"));
    drop(server);
}
