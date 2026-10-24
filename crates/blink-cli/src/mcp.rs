//! `blinkctl mcp`: a small MCP server (standard input and output) with one tool, so any agent that speaks MCP, GUI or terminal,
//! can tell the notch what it is doing. The tool's own description tells the agent when to call it.
//!
//! Protocol: newline-delimited JSON-RPC 2.0. Only what a tool server needs: `initialize`, `ping`, `tools/list`, `tools/call`.

use serde_json::{json, Value};

pub const TOOL: &str = "blink_report_status";
const DEFAULT_PROTOCOL: &str = "2025-06-18";

const DESCRIPTION: &str = "Tell the user's Blink notch (a small status bar at the top of their screen) what you are doing, so they can see it without watching this window. \
Call it with state \"running\" and a short title when you start a task; with \"input\" whenever you stop to ask the user something or need their approval; \
with \"done\" when you have finished; with \"error\" if you gave up. Keep the title under 80 characters. It costs nothing and never fails.";

const INSTRUCTIONS: &str = "Report your status to the user's Blink notch with the blink_report_status tool: running when you start, input when you need the user, done when finished, error on failure.";

/// What a session has told the notch, so the agent keeps one id and is taken off the list when the session ends.
pub struct Session {
    client: String,
    pid: u32,
    pub reported: bool,
}

impl Session {
    pub fn new(pid: u32) -> Session {
        Session { client: String::new(), pid, reported: false }
    }

    /// The id the notch knows this session by.
    pub fn id(&self) -> String {
        let client: String = self.client.chars().filter(|c| c.is_ascii_alphanumeric()).take(20).collect::<String>().to_ascii_lowercase();
        format!("mcp-{}-{}", if client.is_empty() { "agent" } else { &client }, self.pid)
    }

    /// "cursor-vscode" becomes "Cursor Vscode": the client's own name, made readable.
    pub fn display(&self) -> String {
        let words: Vec<String> = self
            .client
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut cs = w.chars();
                cs.next().map(|f| f.to_uppercase().collect::<String>() + cs.as_str()).unwrap_or_default()
            })
            .collect();
        if words.is_empty() { "AI agent".to_string() } else { words.join(" ") }
    }
}

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub fn tools() -> Value {
    json!([{
        "name": TOOL,
        "description": DESCRIPTION,
        "inputSchema": {
            "type": "object",
            "properties": {
                "state": { "type": "string", "enum": ["running", "input", "done", "error", "idle"], "description": "running: working; input: waiting for the user; done: finished; error: gave up; idle: nothing to do" },
                "title": { "type": "string", "description": "What you are doing or need, under 80 characters" },
                "detail": { "type": "string", "description": "An optional second line" },
                "progress": { "type": "number", "minimum": 0, "maximum": 1, "description": "How far along, 0 to 1, if you know" }
            },
            "required": ["state"]
        }
    }])
}

/// Answer one message. `send` delivers an agent update to the notch (`Ok(false)` when it is not running); the answer, if the
/// message wants one, is returned.
pub fn handle(line: &str, session: &mut Session, send: &mut dyn FnMut(Value) -> Result<bool, String>) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Some(error(&Value::Null, -32700, "parse error")),
    };
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let id = message.get("id").cloned();
    // A message without an id is a notification: never answered.
    let Some(id) = id else { return None };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            if let Some(name) = params["clientInfo"]["name"].as_str() {
                session.client = name.to_string();
            }
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            ok(&id, json!({ "protocolVersion": version, "capabilities": { "tools": {} }, "serverInfo": { "name": "blink", "version": env!("CARGO_PKG_VERSION") }, "instructions": INSTRUCTIONS }))
        }
        "ping" => ok(&id, json!({})),
        "tools/list" => ok(&id, json!({ "tools": tools() })),
        "tools/call" => {
            if params["name"].as_str() != Some(TOOL) {
                return Some(error(&id, -32602, "unknown tool"));
            }
            let args = &params["arguments"];
            let mut update = json!({ "cmd": "agent_update", "id": session.id(), "name": session.display() });
            for key in ["state", "title", "detail", "progress"] {
                if let Some(v) = args.get(key) {
                    update[key] = v.clone();
                }
            }
            match send(update) {
                Ok(true) => {
                    session.reported = true;
                    ok(&id, json!({ "content": [{ "type": "text", "text": "Reported to Blink." }] }))
                }
                // Not running: nothing to tell, and no reason to bother the agent with it.
                Ok(false) => ok(&id, json!({ "content": [{ "type": "text", "text": "Blink is not running; nothing was reported." }] })),
                Err(e) => ok(&id, json!({ "isError": true, "content": [{ "type": "text", "text": format!("Blink did not accept that: {e}") }] })),
            }
        }
        _ => error(&id, -32601, "method not found"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(session: &mut Session, request: Value, sent: &mut Vec<Value>, running: bool) -> Option<Value> {
        handle(&request.to_string(), session, &mut |u| {
            sent.push(u);
            Ok(running)
        })
    }

    #[test]
    fn it_introduces_itself_and_lists_one_tool() {
        let mut s = Session::new(77);
        let mut sent = Vec::new();
        let init = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26", "clientInfo": { "name": "cursor-vscode", "version": "1" }, "capabilities": {} } }), &mut sent, true).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26", "the client's version is echoed");
        assert_eq!(init["result"]["serverInfo"]["name"], "blink");
        assert!(init["result"]["capabilities"]["tools"].is_object());
        assert!(ask(&mut s, json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), &mut sent, true).is_none(), "a notification gets no answer");
        let list = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), &mut sent, true).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], TOOL);
        assert!(tools[0]["description"].as_str().unwrap().contains("\"input\""), "the description tells the agent when to call it");
        assert_eq!(ask(&mut s, json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }), &mut sent, true).unwrap()["result"], json!({}));
        assert_eq!(ask(&mut s, json!({ "jsonrpc": "2.0", "id": 4, "method": "nope" }), &mut sent, true).unwrap()["error"]["code"], -32601);
    }

    #[test]
    fn a_call_becomes_an_update_named_for_the_client_and_the_process() {
        let mut s = Session::new(4321);
        let mut sent = Vec::new();
        ask(&mut s, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "claude-ai" } } }), &mut sent, true);
        let answer = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": TOOL, "arguments": { "state": "input", "title": "Approve the migration?", "progress": 0.5 } } }), &mut sent, true).unwrap();
        assert!(answer["result"]["isError"].is_null());
        assert_eq!(sent[0]["cmd"], "agent_update");
        assert_eq!((sent[0]["id"].as_str(), sent[0]["name"].as_str(), sent[0]["state"].as_str(), sent[0]["title"].as_str()), (Some("mcp-claudeai-4321"), Some("Claude Ai"), Some("input"), Some("Approve the migration?")));
        assert!(s.reported);
        assert!(blink_core::agents::valid_id(&s.id()));
    }

    #[test]
    fn a_missing_notch_is_not_the_agents_problem_and_a_refusal_is_said() {
        let mut s = Session::new(1);
        let mut sent = Vec::new();
        let gone = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": TOOL, "arguments": { "state": "done" } } }), &mut sent, false).unwrap();
        assert!(gone["result"]["isError"].is_null() && gone["result"]["content"][0]["text"].as_str().unwrap().contains("not running"));
        assert!(!s.reported);
        let refused = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": TOOL, "arguments": { "state": "dancing" } } }).to_string(), &mut s, &mut |_| Err("BAD_STATE".into())).unwrap();
        assert_eq!(refused["result"]["isError"], true);
        let unknown = ask(&mut s, json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "other" } }), &mut sent, true).unwrap();
        assert_eq!(unknown["error"]["code"], -32602);
        assert_eq!(handle("not json", &mut s, &mut |_| Ok(true)).unwrap()["error"]["code"], -32700);
    }

    #[test]
    fn names_are_made_readable() {
        let mut s = Session::new(1);
        assert_eq!(s.display(), "AI agent");
        s.client = "cursor-vscode".into();
        assert_eq!(s.display(), "Cursor Vscode");
        assert_eq!(s.id(), "mcp-cursorvscode-1");
    }
}
