//! What AI agents (and any other program) tell the notch about themselves: who they are and what they are doing.
//!
//! A program sends an update over the notch's pipe (`blinkctl agent ...`, or the `agent_update` request itself):
//!
//! ```json
//! {"cmd":"agent_update","id":"claude-1","name":"Claude Code","state":"running","title":"Refactor the parser","detail":"3 of 8 files","progress":0.4,"url":"https://..."}
//! ```
//!
//! Everything here is validated and trimmed, because the sender is another program: ids and names are short and plain,
//! a link must be https, the list is small, and nothing is written to disk (agents come and go with the notch).

use std::collections::HashMap;

use serde_json::Value;

/// The most agents the notch keeps. A new one past this replaces the one that has been quiet longest.
pub const MAX_AGENTS: usize = 12;
const MAX_ID: usize = 40;
const MAX_NAME: usize = 40;
const MAX_TITLE: usize = 90;
const MAX_DETAIL: usize = 120;
const MAX_URL: usize = 400;
/// An agent that has not said anything for this long is dropped (a crashed program never says it is done).
pub const FORGET_AFTER_MS: i64 = 6 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    /// Working on something.
    Running,
    /// Waiting for the person: a question, an approval.
    Input,
    /// Finished well.
    Done,
    /// Stopped with a problem.
    Error,
    /// Alive but not doing anything.
    Idle,
}

impl AgentState {
    pub fn parse(word: &str) -> Option<AgentState> {
        match word.trim().to_ascii_lowercase().as_str() {
            "running" | "run" | "working" | "busy" => Some(AgentState::Running),
            "input" | "waiting" | "needs-input" | "ask" => Some(AgentState::Input),
            "done" | "finished" | "complete" | "completed" | "ok" => Some(AgentState::Done),
            "error" | "failed" | "fail" => Some(AgentState::Error),
            "idle" | "sleeping" => Some(AgentState::Idle),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            AgentState::Running => "running",
            AgentState::Input => "input",
            AgentState::Done => "done",
            AgentState::Error => "error",
            AgentState::Idle => "idle",
        }
    }

    /// How much the person is wanted: shown first, and what a peek or the pill is worth.
    pub fn urgency(self) -> u8 {
        match self {
            AgentState::Input => 3,
            AgentState::Error => 3,
            AgentState::Done => 2,
            AgentState::Running => 1,
            AgentState::Idle => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub state: AgentState,
    pub title: String,
    pub detail: String,
    /// 0 to 1, when the agent knows.
    pub progress: Option<f32>,
    pub url: Option<String>,
    /// When it last said anything (wall ms).
    pub updated_ms: i64,
    /// When it entered its current state (wall ms).
    pub since_ms: i64,
}

/// What an update changed, so the notch can decide whether it is worth a peek.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The first word from this agent.
    New,
    /// It went from one state to another.
    State(AgentState),
    /// Same state: a new title, detail or progress.
    Same,
}

/// A request to add or change an agent, already checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub id: String,
    pub name: Option<String>,
    pub state: Option<AgentState>,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub progress: Option<f32>,
    pub url: Option<String>,
}

/// Plain text only: no control characters, trimmed, cut to `max` characters.
fn clean(text: &str, max: usize) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(max).collect()
}

/// Letters, digits and `._-`: an id is something to compare, not to show.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= MAX_ID && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl Update {
    pub fn parse(v: &Value) -> Result<Update, String> {
        let id = v["id"].as_str().map(str::trim).unwrap_or("");
        if !valid_id(id) {
            return Err("BAD_ID".to_string());
        }
        let state = match v.get("state") {
            None | Some(Value::Null) => None,
            Some(s) => Some(s.as_str().and_then(AgentState::parse).ok_or("BAD_STATE")?),
        };
        let text = |key: &str, max: usize| v.get(key).and_then(Value::as_str).map(|t| clean(t, max));
        let url = match v.get("url").and_then(Value::as_str).map(str::trim) {
            None | Some("") => None,
            Some(u) if u.len() <= MAX_URL && u.to_ascii_lowercase().starts_with("https://") && !u.chars().any(|c| c.is_control() || c == ' ') => Some(u.to_string()),
            Some(_) => return Err("BAD_URL".to_string()),
        };
        let progress = match v.get("progress") {
            None | Some(Value::Null) => None,
            Some(p) => match p.as_f64() {
                Some(p) if p.is_finite() => Some(p.clamp(0.0, 1.0) as f32),
                _ => return Err("BAD_PROGRESS".to_string()),
            },
        };
        Ok(Update { id: id.to_string(), name: text("name", MAX_NAME), state, title: text("title", MAX_TITLE), detail: text("detail", MAX_DETAIL), progress, url })
    }
}

#[derive(Debug, Default)]
pub struct Agents {
    list: Vec<Agent>,
}

impl Agents {
    pub fn new() -> Agents {
        Agents::default()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn get(&self, id: &str) -> Option<&Agent> {
        self.list.iter().find(|a| a.id == id)
    }

    /// Apply an update. A new agent takes `Running` unless it says otherwise; a missing field keeps what was there.
    pub fn apply(&mut self, u: Update, wall_ms: i64) -> Change {
        if let Some(at) = self.list.iter().position(|a| a.id == u.id) {
            let a = &mut self.list[at];
            let before = a.state;
            if let Some(name) = u.name.filter(|n| !n.is_empty()) {
                a.name = name;
            }
            if let Some(state) = u.state {
                a.state = state;
            }
            if let Some(title) = u.title {
                a.title = title;
            }
            if let Some(detail) = u.detail {
                a.detail = detail;
            }
            // A new state starts with no progress of its own.
            if a.state != before {
                a.progress = None;
                a.since_ms = wall_ms;
            }
            if u.progress.is_some() {
                a.progress = u.progress;
            }
            if u.url.is_some() {
                a.url = u.url;
            }
            a.updated_ms = wall_ms;
            return if a.state != before { Change::State(a.state) } else { Change::Same };
        }
        if self.list.len() >= MAX_AGENTS {
            if let Some(oldest) = self.list.iter().enumerate().min_by_key(|(_, a)| a.updated_ms).map(|(i, _)| i) {
                self.list.remove(oldest);
            }
        }
        let state = u.state.unwrap_or(AgentState::Running);
        self.list.push(Agent {
            name: u.name.filter(|n| !n.is_empty()).unwrap_or_else(|| u.id.clone()),
            id: u.id,
            state,
            title: u.title.unwrap_or_default(),
            detail: u.detail.unwrap_or_default(),
            progress: u.progress,
            url: u.url,
            updated_ms: wall_ms,
            since_ms: wall_ms,
        });
        Change::New
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|a| a.id != id);
        self.list.len() != before
    }

    /// Drop what has been quiet for too long.
    pub fn forget_old(&mut self, wall_ms: i64) -> bool {
        let before = self.list.len();
        self.list.retain(|a| wall_ms - a.updated_ms < FORGET_AFTER_MS);
        self.list.len() != before
    }

    /// Remove everything finished or failed (the "clear" button).
    pub fn clear_finished(&mut self) -> usize {
        let before = self.list.len();
        self.list.retain(|a| !matches!(a.state, AgentState::Done | AgentState::Error));
        before - self.list.len()
    }

    /// In the order to show: the ones that want the person first, then running, then the rest; the most recent first within each.
    pub fn sorted(&self) -> Vec<&Agent> {
        let mut v: Vec<&Agent> = self.list.iter().collect();
        v.sort_by(|a, b| b.state.urgency().cmp(&a.state.urgency()).then(b.updated_ms.cmp(&a.updated_ms)));
        v
    }

    /// How many agents are in each state.
    pub fn counts(&self) -> HashMap<&'static str, usize> {
        let mut m = HashMap::new();
        for a in &self.list {
            *m.entry(a.state.word()).or_insert(0) += 1;
        }
        m
    }

    /// The agent that most deserves the resting pill: one waiting for the person or failed, the longest waiting first.
    pub fn needs_you(&self) -> Option<&Agent> {
        self.list.iter().filter(|a| matches!(a.state, AgentState::Input | AgentState::Error)).min_by_key(|a| a.since_ms)
    }
}

/// "4s", "12m", "3h": how long ago, coarse.
pub fn ago(from_ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - from_ms) / 1000).max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn update(v: Value) -> Update {
        Update::parse(&v).expect("a good update")
    }

    #[test]
    fn an_update_is_checked_and_cleaned() {
        let u = update(json!({ "id": "claude-1", "name": "  Claude\n Code ", "state": "Working", "title": "Fix\tthe parser", "progress": 3, "url": "https://example.com/run/1" }));
        assert_eq!((u.name.as_deref(), u.state, u.title.as_deref(), u.progress), (Some("Claude Code"), Some(AgentState::Running), Some("Fix the parser"), Some(1.0)));
        assert_eq!(Update::parse(&json!({ "id": "has space" })), Err("BAD_ID".into()));
        assert_eq!(Update::parse(&json!({ "id": "" })), Err("BAD_ID".into()));
        assert_eq!(Update::parse(&json!({ "id": "a".repeat(41) })), Err("BAD_ID".into()));
        assert_eq!(Update::parse(&json!({ "id": "a", "state": "dancing" })), Err("BAD_STATE".into()));
        assert_eq!(Update::parse(&json!({ "id": "a", "url": "http://insecure" })), Err("BAD_URL".into()));
        assert_eq!(Update::parse(&json!({ "id": "a", "url": "file:///etc/passwd" })), Err("BAD_URL".into()));
        assert_eq!(Update::parse(&json!({ "id": "a", "progress": "half" })), Err("BAD_PROGRESS".into()));
        let long = update(json!({ "id": "a", "title": "x".repeat(500) }));
        assert_eq!(long.title.map(|t| t.chars().count()), Some(MAX_TITLE));
    }

    #[test]
    fn a_new_agent_runs_unless_told_otherwise_and_updates_keep_what_they_do_not_mention() {
        let mut agents = Agents::new();
        assert_eq!(agents.apply(update(json!({ "id": "a", "name": "Agent A", "title": "Build" })), 1_000), Change::New);
        assert_eq!(agents.get("a").map(|a| a.state), Some(AgentState::Running));
        assert_eq!(agents.apply(update(json!({ "id": "a", "progress": 0.5 })), 2_000), Change::Same);
        let a = agents.get("a").unwrap();
        assert_eq!((a.name.as_str(), a.title.as_str(), a.progress, a.since_ms, a.updated_ms), ("Agent A", "Build", Some(0.5), 1_000, 2_000));
        assert_eq!(agents.apply(update(json!({ "id": "a", "state": "input", "detail": "Run the tests?" })), 3_000), Change::State(AgentState::Input));
        let a = agents.get("a").unwrap();
        assert_eq!((a.progress, a.since_ms, a.detail.as_str()), (None, 3_000, "Run the tests?"), "a new state starts without the old progress");
    }

    #[test]
    fn the_list_is_sorted_for_the_person_and_kept_small() {
        let mut agents = Agents::new();
        for (id, state, at) in [("run", "running", 5), ("done", "done", 9), ("ask", "input", 1), ("bad", "error", 2), ("nap", "idle", 8)] {
            agents.apply(update(json!({ "id": id, "state": state })), at);
        }
        let order: Vec<&str> = agents.sorted().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(order, ["bad", "ask", "done", "run", "nap"], "{order:?}");
        assert_eq!(agents.needs_you().map(|a| a.id.as_str()), Some("ask"), "the one waiting longest");
        for i in 0..20 {
            agents.apply(update(json!({ "id": format!("n{i}") })), 100 + i);
        }
        assert_eq!(agents.len(), MAX_AGENTS);
        assert!(agents.get("n19").is_some() && agents.get("ask").is_none(), "the quietest were replaced");
    }

    #[test]
    fn finished_ones_clear_and_old_ones_are_forgotten() {
        let mut agents = Agents::new();
        agents.apply(update(json!({ "id": "a", "state": "done" })), 0);
        agents.apply(update(json!({ "id": "b", "state": "running" })), 0);
        assert_eq!(agents.clear_finished(), 1);
        assert!(agents.get("a").is_none() && agents.get("b").is_some());
        assert!(!agents.forget_old(FORGET_AFTER_MS - 1));
        assert!(agents.forget_old(FORGET_AFTER_MS + 1) && agents.is_empty());
        assert!(!agents.remove("b"));
    }

    #[test]
    fn how_long_ago_is_coarse() {
        assert_eq!(ago(0, 4_000), "4s");
        assert_eq!(ago(0, 12 * 60_000), "12m");
        assert_eq!(ago(0, 3 * 3_600_000), "3h");
        assert_eq!(ago(10, 5), "0s");
    }
}
