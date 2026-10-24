//! Turning what an agent's own hooks say into agent updates, so one command (`blinkctl hook <dialect>`) serves every event
//! of an agent: the hook gives the event name and the session on standard input, and this decides what it means.
//!
//! A dialect is one agent's way of naming things. Adding one is a table here, not a new program.

use serde_json::{json, Value};

/// What a hook event comes to.
#[derive(Debug, PartialEq)]
pub enum HookAction {
    /// An `agent_update` request.
    Update(Value),
    /// The session is over: take the agent off the list.
    Remove(String),
    /// Nothing to tell the notch.
    Ignore,
}

/// The dialects `blinkctl hook` knows.
pub const DIALECTS: [&str; 2] = ["claude", "gemini"];

/// What a hook must print on standard output for its agent to be satisfied (Gemini's hooks want a JSON object).
pub fn stdout_for(dialect: &str) -> Option<&'static str> {
    (dialect == "gemini").then_some("{}")
}

fn text<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty())
}

/// The last folder of a path, either slash.
fn folder_name(path: &str) -> Option<String> {
    path.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().filter(|n| !n.is_empty()).map(str::to_string)
}

/// An id that stays the same for one session of one agent: letters and digits of the session id, a few of them.
fn session_id(prefix: &str, input: &Value) -> String {
    let short: String = text(input, "session_id").unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    if short.is_empty() { prefix.to_string() } else { format!("{prefix}-{short}") }
}

fn display_name(product: &str, input: &Value) -> String {
    match text(input, "cwd").and_then(folder_name) {
        Some(project) => format!("{product} · {project}"),
        None => product.to_string(),
    }
}

fn update(id: String, name: String, state: &str, title: &str) -> HookAction {
    HookAction::Update(json!({ "cmd": "agent_update", "id": id, "name": name, "state": state, "title": title }))
}

/// What one hook event means for the notch. `input` is the JSON the agent gave the hook.
pub fn from_hook(dialect: &str, input: &Value) -> Result<HookAction, String> {
    match dialect {
        "claude" => Ok(claude(input)),
        "gemini" => Ok(gemini(input)),
        other => Err(format!("unknown dialect {other} (known: {})", DIALECTS.join(", "))),
    }
}

fn claude(input: &Value) -> HookAction {
    let id = session_id("claude", input);
    let name = display_name("Claude Code", input);
    match text(input, "hook_event_name").unwrap_or("") {
        "UserPromptSubmit" | "PostToolUse" => update(id, name, "running", "Working"),
        "SessionStart" => update(id, name, "idle", "Session started"),
        // Waiting for the person: a question, an idle prompt, a permission.
        "Notification" => update(id, name, "input", text(input, "message").unwrap_or("Needs your attention")),
        "PermissionRequest" => {
            let title = match text(input, "tool_name") {
                Some(tool) => format!("Allow {tool}?"),
                None => "Needs your permission".to_string(),
            };
            update(id, name, "input", &title)
        }
        "Stop" => update(id, name, "done", "Finished"),
        "StopFailure" => update(id, name, "error", "Stopped with an error"),
        "SessionEnd" => HookAction::Remove(id),
        _ => HookAction::Ignore,
    }
}

fn gemini(input: &Value) -> HookAction {
    let id = session_id("gemini", input);
    let name = display_name("Gemini CLI", input);
    match text(input, "hook_event_name").unwrap_or("") {
        "BeforeAgent" => update(id, name, "running", "Working"),
        "AfterAgent" => update(id, name, "done", "Finished"),
        "SessionStart" => update(id, name, "idle", "Session started"),
        "Notification" => update(id, name, "input", text(input, "message").unwrap_or("Needs your attention")),
        "SessionEnd" => HookAction::Remove(id),
        _ => HookAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str) -> Value {
        json!({ "hook_event_name": name, "session_id": "ab12-cd34-ef56", "cwd": "C:\\Users\\me\\projects\\parser" })
    }

    #[test]
    fn claude_events_become_states_with_the_session_and_the_project_in_the_name() {
        let HookAction::Update(u) = from_hook("claude", &event("Stop")).unwrap() else { panic!("an update") };
        assert_eq!((u["id"].as_str(), u["name"].as_str(), u["state"].as_str(), u["title"].as_str()), (Some("claude-ab12cd34"), Some("Claude Code · parser"), Some("done"), Some("Finished")));
        let state = |name: &str| match from_hook("claude", &event(name)).unwrap() {
            HookAction::Update(u) => u["state"].as_str().unwrap().to_string(),
            HookAction::Remove(_) => "remove".to_string(),
            HookAction::Ignore => "ignore".to_string(),
        };
        assert_eq!(state("UserPromptSubmit"), "running");
        assert_eq!(state("PostToolUse"), "running");
        assert_eq!(state("Notification"), "input");
        assert_eq!(state("PermissionRequest"), "input");
        assert_eq!(state("StopFailure"), "error");
        assert_eq!(state("SessionStart"), "idle");
        assert_eq!(state("SessionEnd"), "remove");
        assert_eq!(state("SubagentStop"), "ignore");
    }

    #[test]
    fn a_notification_says_what_the_agent_said_and_a_permission_names_the_tool() {
        let mut n = event("Notification");
        n["message"] = json!("Claude needs your permission to use Bash");
        let HookAction::Update(u) = from_hook("claude", &n).unwrap() else { panic!() };
        assert_eq!(u["title"], "Claude needs your permission to use Bash");
        let mut p = event("PermissionRequest");
        p["tool_name"] = json!("Bash");
        let HookAction::Update(u) = from_hook("claude", &p).unwrap() else { panic!() };
        assert_eq!(u["title"], "Allow Bash?");
    }

    #[test]
    fn two_sessions_are_two_agents_and_a_missing_session_still_works() {
        let mut a = event("Stop");
        a["session_id"] = json!("zzzz9999");
        let (HookAction::Update(x), HookAction::Update(y)) = (from_hook("claude", &event("Stop")).unwrap(), from_hook("claude", &a).unwrap()) else { panic!() };
        assert_ne!(x["id"], y["id"]);
        let HookAction::Update(z) = from_hook("claude", &json!({ "hook_event_name": "Stop" })).unwrap() else { panic!() };
        assert_eq!((z["id"].as_str(), z["name"].as_str()), (Some("claude"), Some("Claude Code")));
        // The id always passes the notch's own check.
        assert!(crate::agents::valid_id(x["id"].as_str().unwrap()));
    }

    #[test]
    fn gemini_has_its_own_names_and_wants_json_back() {
        let state = |name: &str| match from_hook("gemini", &event(name)).unwrap() {
            HookAction::Update(u) => u["state"].as_str().unwrap().to_string(),
            HookAction::Remove(_) => "remove".to_string(),
            HookAction::Ignore => "ignore".to_string(),
        };
        assert_eq!((state("BeforeAgent").as_str(), state("AfterAgent").as_str(), state("Notification").as_str(), state("SessionEnd").as_str()), ("running", "done", "input", "remove"));
        assert_eq!(stdout_for("gemini"), Some("{}"));
        assert_eq!(stdout_for("claude"), None);
    }

    #[test]
    fn an_unknown_dialect_is_an_error_and_unknown_events_are_ignored() {
        assert!(from_hook("nope", &json!({})).is_err());
        assert_eq!(from_hook("claude", &json!({})).unwrap(), HookAction::Ignore);
    }
}
