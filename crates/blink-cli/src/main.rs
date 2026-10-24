//! `blinkctl`: tell the running notch things from a terminal, a script or an AI agent's hook.
//!
//!   blinkctl agent --id ID [--name NAME] [--state STATE] [--title TEXT] [--detail TEXT] [--progress 0..1] [--url https://...]
//!   blinkctl agent --id ID --remove
//!   blinkctl agent --list
//!   blinkctl agent --json            read one JSON update per line from standard input and forward each
//!   blinkctl ping
//!
//! STATE is running, input (waiting for you), done, error or idle. Add `--quiet` to exit 0 silently when the notch is not
//! running (for hooks that must never fail). Exit codes: 0 done, 2 wrong use, 3 the notch is not running, 4 the notch
//! refused (the reason is printed).

use std::io::BufRead;
use std::process::exit;

use blink_core::ipc::{self, IpcError};
use serde_json::{json, Map, Value};

const USAGE: &str = "usage:
  blinkctl agent --id ID [--name NAME] [--state running|input|done|error|idle] [--title TEXT] [--detail TEXT] [--progress 0..1] [--url https://...] [--quiet]
  blinkctl agent --id ID --remove
  blinkctl agent --list
  blinkctl agent --json        (one JSON update per line on standard input)
  blinkctl ping";

fn fail(code: i32, message: &str) -> ! {
    eprintln!("blinkctl: {message}");
    exit(code);
}

/// What the flags after `agent` ask for.
#[derive(Debug, PartialEq)]
enum AgentCall {
    Update(Value),
    Remove(String),
    List,
    Stdin,
}

#[derive(Debug, PartialEq)]
struct Parsed {
    call: AgentCall,
    quiet: bool,
}

fn parse_agent(args: &[String]) -> Result<Parsed, String> {
    let mut fields = Map::new();
    let (mut remove, mut list, mut stdin, mut quiet) = (false, false, false, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--id" => {
                fields.insert("id".into(), Value::String(value("--id")?));
            }
            "--name" => {
                fields.insert("name".into(), Value::String(value("--name")?));
            }
            "--state" => {
                fields.insert("state".into(), Value::String(value("--state")?));
            }
            "--title" => {
                fields.insert("title".into(), Value::String(value("--title")?));
            }
            "--detail" => {
                fields.insert("detail".into(), Value::String(value("--detail")?));
            }
            "--url" => {
                fields.insert("url".into(), Value::String(value("--url")?));
            }
            "--progress" => {
                let p: f64 = value("--progress")?.parse().map_err(|_| "--progress is a number from 0 to 1".to_string())?;
                fields.insert("progress".into(), json!(p));
            }
            "--remove" => remove = true,
            "--list" => list = true,
            "--json" => stdin = true,
            "--quiet" | "-q" => quiet = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    let call = if list {
        AgentCall::List
    } else if stdin {
        AgentCall::Stdin
    } else if remove {
        AgentCall::Remove(fields.get("id").and_then(Value::as_str).ok_or("--remove needs --id")?.to_string())
    } else {
        if !fields.contains_key("id") {
            return Err("--id is needed".to_string());
        }
        fields.insert("cmd".into(), Value::String("agent_update".into()));
        AgentCall::Update(Value::Object(fields))
    };
    Ok(Parsed { call, quiet })
}

/// Send one request. `quiet` turns "no notch running" into a quiet success.
fn send(request: &Value, quiet: bool) -> Value {
    match ipc::request(request) {
        Ok(answer) => answer,
        Err(IpcError::NotRunning) if quiet => exit(0),
        Err(IpcError::NotRunning) => fail(3, "Blink is not running"),
        Err(IpcError::Failed(code)) => fail(4, &format!("the notch refused: {code}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("ping") => {
            let answer = send(&json!({ "cmd": "ping" }), false);
            println!("Blink {}", answer["version"].as_str().unwrap_or("?"));
        }
        Some("agent") => {
            let parsed = parse_agent(&args[1..]).unwrap_or_else(|e| fail(2, &format!("{e}\n{USAGE}")));
            match parsed.call {
                AgentCall::Update(update) => {
                    send(&update, parsed.quiet);
                }
                AgentCall::Remove(id) => {
                    send(&json!({ "cmd": "agent_remove", "id": id }), parsed.quiet);
                }
                AgentCall::List => {
                    let answer = send(&json!({ "cmd": "agent_list" }), parsed.quiet);
                    for a in answer["agents"].as_array().into_iter().flatten() {
                        println!("{:<24} {:<8} {}", a["id"].as_str().unwrap_or(""), a["state"].as_str().unwrap_or(""), a["title"].as_str().unwrap_or(""));
                    }
                }
                AgentCall::Stdin => {
                    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(line) {
                            Ok(Value::Object(mut update)) => {
                                update.insert("cmd".into(), Value::String("agent_update".into()));
                                send(&Value::Object(update), parsed.quiet);
                            }
                            _ => fail(2, "each line must be a JSON object"),
                        }
                    }
                }
            }
        }
        _ => fail(2, USAGE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_update_collects_its_flags() {
        let p = parse_agent(&args(&["--id", "a1", "--state", "input", "--title", "Run the tests?", "--progress", "0.5", "-q"])).unwrap();
        assert!(p.quiet);
        assert_eq!(p.call, AgentCall::Update(json!({ "cmd": "agent_update", "id": "a1", "state": "input", "title": "Run the tests?", "progress": 0.5 })));
    }

    #[test]
    fn other_calls_and_mistakes() {
        assert_eq!(parse_agent(&args(&["--id", "a1", "--remove"])).unwrap().call, AgentCall::Remove("a1".into()));
        assert_eq!(parse_agent(&args(&["--list"])).unwrap().call, AgentCall::List);
        assert_eq!(parse_agent(&args(&["--json"])).unwrap().call, AgentCall::Stdin);
        assert!(parse_agent(&args(&["--state", "done"])).is_err(), "an update needs an id");
        assert!(parse_agent(&args(&["--remove"])).is_err());
        assert!(parse_agent(&args(&["--id"])).is_err());
        assert!(parse_agent(&args(&["--id", "a", "--progress", "lots"])).is_err());
        assert!(parse_agent(&args(&["--frobnicate"])).is_err());
    }
}
