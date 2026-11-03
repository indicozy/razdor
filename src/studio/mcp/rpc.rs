//! JSON-RPC 2.0 framing and the MCP methods the server answers: `initialize`, `ping`, `tools/list`,
//! `tools/call`. One message per line (the stdio transport's framing, and the TCP one's). A
//! notification (no `id`) gets no reply; a tool that fails gets a result with `isError`, as MCP asks,
//! and only an unknown tool or method is a JSON-RPC error.

use serde_json::{json, Value};

use super::host::Host;
use super::{tools, PROTOCOL_VERSION, SERVER_NAME};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// What the agent reads first (the `initialize` reply's `instructions`).
pub const INSTRUCTIONS: &str = "Razdor Studio: tools to see and test the package the studio has open. \
Edit the package's files yourself; the studio picks the changes up. Start with get_state, run check after \
each edit, snapshot to look at the map, and sim to play the story.";

/// One incoming line, answered: the reply's text, or `None` for a notification or a client's reply.
pub fn handle_line(host: &dyn Host, line: &str) -> Option<String> {
    handle(host, line).map(|reply| reply.to_string())
}

/// One incoming message, answered as a JSON value (see [`handle_line`]).
pub fn handle(host: &dyn Host, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, PARSE_ERROR, &format!("parse error: {e}"))),
    };
    let Some(obj) = msg.as_object() else {
        return Some(error(Value::Null, INVALID_REQUEST, "a JSON-RPC message is an object (batches are not supported)"));
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A reply of the client to a request of ours: nothing to answer. Anything else is invalid.
        if obj.contains_key("result") || obj.contains_key("error") {
            return None;
        }
        return Some(error(obj.get("id").cloned().unwrap_or(Value::Null), INVALID_REQUEST, "the message has no method"));
    };
    // No `id` is a notification: its method is done (or ignored), and nothing goes back.
    let id = obj.get("id")?.clone();
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => result(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }),
        ),
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tools::list() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, INVALID_PARAMS, "tools/call needs a tool name"));
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match tools::call(host, name, &args) {
                Some(r) => result(id, r),
                None => error(id, INVALID_PARAMS, &format!("unknown tool `{name}`")),
            }
        }
        other => error(id, METHOD_NOT_FOUND, &format!("method not found: {other}")),
    })
}

/// A JSON-RPC result reply.
pub fn result(id: Value, value: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": value })
}

/// A JSON-RPC error reply.
pub fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::build::Job;
    use crate::studio::mcp::host::{FocusTarget, Frame, SnapTarget};

    /// A host with no window: every window-only call fails.
    struct NoWindow;

    impl Host for NoWindow {
        fn job(&self) -> Result<Job, String> {
            Err("no window".into())
        }
        fn state(&self) -> Result<Value, String> {
            Err("no window".into())
        }
        fn focus(&self, _: FocusTarget) -> Result<Value, String> {
            Err("no window".into())
        }
        fn snapshot(&self, _: SnapTarget) -> Result<Frame, String> {
            Err("no window".into())
        }
    }

    fn reply(line: &str) -> Value {
        handle(&NoWindow, line).expect("a reply")
    }

    #[test]
    fn initialize_answers_with_the_protocol_version_and_the_tools_capability() {
        let r = reply(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#);
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "razdor-studio");
        assert!(r["result"]["instructions"].as_str().unwrap().contains("get_state"));
    }

    #[test]
    fn notifications_get_no_reply_and_replies_from_the_client_neither() {
        assert_eq!(handle(&NoWindow, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
        assert_eq!(handle(&NoWindow, r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}"#), None);
        assert_eq!(handle(&NoWindow, r#"{"jsonrpc":"2.0","id":9,"result":{}}"#), None);
    }

    #[test]
    fn ping_is_an_empty_result() {
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#)["result"], json!({}));
    }

    #[test]
    fn framing_errors_and_unknown_methods_are_json_rpc_errors() {
        let parse = reply("{not json");
        assert_eq!(parse["error"]["code"], PARSE_ERROR);
        assert_eq!(parse["id"], Value::Null);
        assert_eq!(reply("[1,2]")["error"]["code"], INVALID_REQUEST);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":2}"#)["error"]["code"], INVALID_REQUEST);
        let unknown = reply(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#);
        assert_eq!((unknown["id"].clone(), unknown["error"]["code"].clone()), (json!(4), json!(METHOD_NOT_FOUND)));
    }

    #[test]
    fn an_unknown_tool_is_invalid_params_and_a_failed_tool_is_an_error_result() {
        let unknown = reply(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"edit_file"}}"#);
        assert_eq!(unknown["error"]["code"], INVALID_PARAMS);
        let failed = reply(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"get_state","arguments":{}}}"#);
        assert_eq!(failed["result"]["isError"], true);
        assert!(failed["result"]["content"][0]["text"].as_str().unwrap().contains("no window"));
    }
}
