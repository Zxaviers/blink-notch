//! Connectors: how Blink gets an AI agent (or any program) to report to the notch, described as data.
//!
//! Every agent is different: some have hooks, some load plugins, some only speak MCP, some have nothing. A connector is a small
//! JSON manifest that says where the agent keeps its settings and what to put there. Blink ships a few and reads more from
//! `connectors/*.json` in its data folder, so supporting another agent needs a file, not a release.
//!
//! ```json
//! { "id": "claude-code", "name": "Claude Code", "description": "...", "verified": true, "docs": "https://...",
//!   "detect": ["%USERPROFILE%\\.claude"],
//!   "install": { "kind": "json-hooks", "files": ["%USERPROFILE%\\.claude\\settings.json"], "create_if_missing": true,
//!                "command": "{{blinkctl}} hook claude", "events": { "Stop": null, "Notification": null } } }
//! ```
//!
//! Kinds: `json-hooks` (hook entries in a settings file), `json-mcp` (an MCP server entry in a config file), `file` (one file
//! written whole, for plugin systems) and `snippet` (text to copy: nothing is written). Blink only writes when the person
//! presses Connect, keeps one backup of a file the first time, and removes only what it added.
//!
//! In paths `%NAME%` is an environment variable (`BLINK_HOME` stands in for `%USERPROFILE%` when set, for tests); in commands
//! and contents `{{blinkctl}}` is the quoted path of `blinkctl.exe` with forward slashes (safe in cmd, PowerShell and bash)
//! and `{{blinkctl_path}}` the same unquoted.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// Text in a hook command or a plugin that marks it as Blink's, so removing never touches anything else.
pub const MARKER: &str = "blink-agent-bridge";

#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    JsonHooks { files: Vec<String>, create_if_missing: bool, command: String, events: Vec<(String, Option<String>)>, handler: Map<String, Value> },
    JsonMcp { files: Vec<String>, servers_key: String, server_name: String, args: Vec<String> },
    File { path: String, content: String },
    Snippet { text: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Connector {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Checked against the agent's own documentation (false: written from what is known, say so).
    pub verified: bool,
    pub docs: Option<String>,
    pub detect: Vec<String>,
    pub install: Install,
    /// From the user's own folder rather than shipped.
    pub user: bool,
}

/// Where things are on this PC.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub blinkctl: PathBuf,
    /// How `blinkctl.exe` is written inside a shell command line (see `Ctx::new`).
    cmd: String,
}

impl Ctx {
    /// `short` is the 8.3 form of the path when Windows has one: a path with a space in it is risky in a hook command, because the
    /// agent may run it in cmd, PowerShell or bash, and each quotes differently. Without a space nothing needs quoting.
    pub fn new(blinkctl: PathBuf, short: Option<String>) -> Ctx {
        let long = blinkctl.to_string_lossy().replace('\\', "/");
        let cmd = if !long.contains(' ') {
            long
        } else if let Some(short) = short.map(|s| s.replace('\\', "/")).filter(|s| !s.contains(' ')) {
            short
        } else {
            format!("\"{long}\"")
        };
        Ctx { blinkctl, cmd }
    }

    fn blinkctl_path(&self) -> String {
        self.blinkctl.to_string_lossy().replace('\\', "/")
    }

    fn fill(&self, text: &str) -> String {
        text.replace("{{blinkctl_path}}", &self.blinkctl_path()).replace("{{blinkctl}}", &self.cmd).replace("{{marker}}", MARKER)
    }
}

/// `%NAME%` replaced by the environment variable; an unknown one stays as written (so the path simply does not exist).
pub fn expand(path: &str) -> PathBuf {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                let value = if name.eq_ignore_ascii_case("USERPROFILE") { std::env::var("BLINK_HOME").ok().filter(|v| !v.is_empty()).or_else(|| std::env::var(name).ok()) } else { std::env::var(name).ok() };
                match value {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
                break;
            }
        }
    }
    out.push_str(rest);
    PathBuf::from(out)
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

impl Connector {
    pub fn parse(text: &str, user: bool) -> Result<Connector, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        let get = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let id = get("id").filter(|i| !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).ok_or("a connector needs an id (letters, digits, - and _)")?;
        let name = get("name").filter(|n| !n.is_empty()).ok_or("a connector needs a name")?;
        let install = v.get("install").ok_or("a connector needs an install")?;
        let kind = install.get("kind").and_then(Value::as_str).ok_or("install needs a kind")?;
        let files = || strings(install.get("files").unwrap_or(&Value::Null));
        let install = match kind {
            "json-hooks" => {
                let events: Vec<(String, Option<String>)> = install
                    .get("events")
                    .and_then(Value::as_object)
                    .ok_or("json-hooks needs events")?
                    .iter()
                    .map(|(k, m)| (k.clone(), m.as_str().map(str::to_string)))
                    .collect();
                if events.is_empty() || files().is_empty() {
                    return Err("json-hooks needs files and events".into());
                }
                Install::JsonHooks {
                    files: files(),
                    create_if_missing: install.get("create_if_missing").and_then(Value::as_bool).unwrap_or(false),
                    command: install.get("command").and_then(Value::as_str).ok_or("json-hooks needs a command")?.to_string(),
                    events,
                    handler: install.get("handler").and_then(Value::as_object).cloned().unwrap_or_default(),
                }
            }
            "json-mcp" => {
                if files().is_empty() {
                    return Err("json-mcp needs files".into());
                }
                Install::JsonMcp {
                    files: files(),
                    servers_key: install.get("servers_key").and_then(Value::as_str).unwrap_or("mcpServers").to_string(),
                    server_name: install.get("server_name").and_then(Value::as_str).unwrap_or("blink").to_string(),
                    args: install.get("args").map(strings).unwrap_or_else(|| vec!["mcp".to_string()]),
                }
            }
            "file" => Install::File {
                path: install.get("path").and_then(Value::as_str).ok_or("file needs a path")?.to_string(),
                content: install.get("content").and_then(Value::as_str).ok_or("file needs content")?.to_string(),
            },
            "snippet" => Install::Snippet { text: install.get("text").and_then(Value::as_str).ok_or("snippet needs text")?.to_string() },
            other => return Err(format!("unknown kind {other}")),
        };
        let docs = get("docs").filter(|d| d.to_ascii_lowercase().starts_with("https://"));
        Ok(Connector { id, name, description: get("description").unwrap_or_default(), verified: v.get("verified").and_then(Value::as_bool).unwrap_or(false), docs, detect: strings(v.get("detect").unwrap_or(&Value::Null)), install, user })
    }

    /// What can be copied instead of connected (the snippet itself, or the MCP entry for a file Blink does not know).
    pub fn snippet(&self, ctx: &Ctx) -> Option<String> {
        match &self.install {
            Install::Snippet { text } => Some(ctx.fill(text)),
            Install::JsonMcp { servers_key, server_name, args, .. } => Some(serde_json::to_string_pretty(&json!({ servers_key: { server_name: { "command": ctx.blinkctl_path(), "args": args } } })).unwrap_or_default()),
            _ => None,
        }
    }

    /// Whether Blink can write for this connector (the others are for copying).
    pub fn installable(&self) -> bool {
        !matches!(self.install, Install::Snippet { .. })
    }
}

// ------------------------------------------------------------------- the shipped connectors

const BUILTIN: [&str; 9] = [
    include_str!("../../../connectors/claude-code.json"),
    include_str!("../../../connectors/gemini-cli.json"),
    include_str!("../../../connectors/opencode.json"),
    include_str!("../../../connectors/cursor.json"),
    include_str!("../../../connectors/windsurf.json"),
    include_str!("../../../connectors/claude-desktop.json"),
    include_str!("../../../connectors/antigravity.json"),
    include_str!("../../../connectors/any-terminal-agent.json"),
    include_str!("../../../connectors/any-script.json"),
];

pub fn builtin() -> Vec<Connector> {
    BUILTIN.iter().filter_map(|t| Connector::parse(t, false).ok()).collect()
}

/// The shipped ones, then the person's own (`connectors/*.json` in the data folder); an own file with a shipped id replaces it,
/// and a file that does not parse is skipped.
pub fn all(user_dir: Option<&Path>) -> Vec<Connector> {
    let mut list = builtin();
    if let Some(dir) = user_dir {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
        files.sort();
        for path in files {
            if let Some(c) = std::fs::read_to_string(&path).ok().and_then(|t| Connector::parse(&t, true).ok()) {
                list.retain(|x| x.id != c.id);
                list.push(c);
            }
        }
    }
    list
}

// ------------------------------------------------------------------- the settings documents

/// Whether a hook command is one of Blink's.
fn ours(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    c.contains("blinkctl") && c.contains(" hook ")
}

/// Add Blink's hook entries (once each) to a settings document.
pub fn hooks_add(doc: &mut Value, events: &[(String, Option<String>)], command: &str, handler: &Map<String, Value>) -> Result<(), String> {
    hooks_remove(doc);
    let root = doc.as_object_mut().ok_or("the settings are not a JSON object")?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or("\"hooks\" is not an object")?;
    for (event, matcher) in events {
        let list = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let list = list.as_array_mut().ok_or_else(|| format!("hooks.{event} is not a list"))?;
        let mut entry = json!({ "type": "command", "command": command });
        for (k, v) in handler {
            entry[k] = v.clone();
        }
        let mut group = json!({ "hooks": [entry] });
        if let Some(m) = matcher {
            group["matcher"] = json!(m);
        }
        list.push(group);
    }
    Ok(())
}

/// Take Blink's hook entries out, and the empty groups and lists they leave. Returns how many entries went.
pub fn hooks_remove(doc: &mut Value) -> usize {
    let mut removed = 0;
    let Some(hooks) = doc.get_mut("hooks").and_then(Value::as_object_mut) else { return 0 };
    for (_, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else { continue };
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = handlers.len();
                handlers.retain(|h| !h.get("command").and_then(Value::as_str).is_some_and(ours));
                removed += before - handlers.len();
            }
        }
        groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|h| !h.is_empty()));
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        if let Some(root) = doc.as_object_mut() {
            root.remove("hooks");
        }
    }
    removed
}

pub fn hooks_present(doc: &Value) -> bool {
    doc.get("hooks").and_then(Value::as_object).is_some_and(|h| {
        h.values().filter_map(Value::as_array).flatten().filter_map(|g| g.get("hooks").and_then(Value::as_array)).flatten().any(|e| e.get("command").and_then(Value::as_str).is_some_and(ours))
    })
}

pub fn mcp_add(doc: &mut Value, servers_key: &str, server_name: &str, command: &str, args: &[String]) -> Result<(), String> {
    let root = doc.as_object_mut().ok_or("the config is not a JSON object")?;
    let servers = root.entry(servers_key.to_string()).or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or_else(|| format!("\"{servers_key}\" is not an object"))?;
    servers.insert(server_name.to_string(), json!({ "command": command, "args": args }));
    Ok(())
}

pub fn mcp_remove(doc: &mut Value, servers_key: &str, server_name: &str) -> bool {
    doc.get_mut(servers_key).and_then(Value::as_object_mut).is_some_and(|s| s.remove(server_name).is_some())
}

pub fn mcp_present(doc: &Value, servers_key: &str, server_name: &str) -> bool {
    doc.get(servers_key).and_then(|s| s.get(server_name)).is_some()
}

// ------------------------------------------------------------------- the files

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// The agent seems to be installed here.
    pub found: bool,
    pub connected: bool,
    /// The file Blink would write (or did).
    pub target: Option<PathBuf>,
}

fn read_json(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text).map_err(|_| format!("{} is not plain JSON (comments or a typo), so Blink does not touch it", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Write a file the safe way: one backup of what was there the first time, then a temporary file renamed over it.
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    if path.exists() {
        let backup = PathBuf::from(format!("{}.blink-backup", path.display()));
        if !backup.exists() {
            std::fs::copy(path, &backup).map_err(|e| format!("cannot back up {}: {e}", path.display()))?;
        }
    }
    let temp = PathBuf::from(format!("{}.blink-tmp", path.display()));
    std::fs::write(&temp, bytes).map_err(|e| format!("cannot write {}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

fn write_json(path: &Path, doc: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    text.push('\n');
    write_file(path, text.as_bytes())
}

/// The file to read and write: the first candidate that exists, else (when allowed) the first one whose folder exists.
fn pick(files: &[String], create_if_missing: bool) -> Option<PathBuf> {
    let paths: Vec<PathBuf> = files.iter().map(|f| expand(f)).collect();
    paths.iter().find(|p| p.exists()).cloned().or_else(|| {
        if create_if_missing {
            paths.iter().find(|p| p.parent().is_some_and(Path::exists)).cloned()
        } else {
            None
        }
    })
}

pub fn status(c: &Connector) -> Status {
    let found_by_detect = c.detect.iter().any(|d| expand(d).exists());
    match &c.install {
        Install::JsonHooks { files, create_if_missing, .. } => {
            let target = pick(files, *create_if_missing);
            let connected = target.as_deref().and_then(|t| read_json(t).ok()).is_some_and(|d| hooks_present(&d));
            Status { found: found_by_detect || target.is_some(), connected, target }
        }
        Install::JsonMcp { files, servers_key, server_name, .. } => {
            let target = pick(files, false);
            let connected = target.as_deref().and_then(|t| read_json(t).ok()).is_some_and(|d| mcp_present(&d, servers_key, server_name));
            Status { found: found_by_detect || target.is_some(), connected, target }
        }
        Install::File { path, .. } => {
            let target = expand(path);
            let connected = std::fs::read_to_string(&target).is_ok_and(|t| t.contains(MARKER));
            Status { found: found_by_detect || target.parent().is_some_and(Path::exists), connected, target: Some(target) }
        }
        Install::Snippet { .. } => Status { found: false, connected: false, target: None },
    }
}

/// Connect: write what the manifest says. Returns what was written, for the person to read.
pub fn connect(c: &Connector, ctx: &Ctx) -> Result<String, String> {
    match &c.install {
        Install::JsonHooks { files, create_if_missing, command, events, handler } => {
            let target = pick(files, *create_if_missing).ok_or_else(|| format!("{} does not seem to be installed here (no settings file found)", c.name))?;
            let mut doc = read_json(&target)?;
            // The marker rides along as an argument the agent's hook ignores, so removal can find these entries.
            hooks_add(&mut doc, events, &ctx.fill(command), handler)?;
            write_json(&target, &doc)?;
            Ok(format!("Hooks added to {}", target.display()))
        }
        Install::JsonMcp { files, servers_key, server_name, args } => {
            let target = pick(files, false).ok_or_else(|| format!("{} has no MCP config file here yet: copy the entry instead", c.name))?;
            let mut doc = read_json(&target)?;
            mcp_add(&mut doc, servers_key, server_name, &ctx.blinkctl_path(), args)?;
            write_json(&target, &doc)?;
            Ok(format!("The Blink MCP server was added to {}", target.display()))
        }
        Install::File { path, content } => {
            let target = expand(path);
            write_file(&target, ctx.fill(content).as_bytes())?;
            Ok(format!("Wrote {}", target.display()))
        }
        Install::Snippet { .. } => Err("this one is for copying".to_string()),
    }
}

pub fn disconnect(c: &Connector) -> Result<String, String> {
    match &c.install {
        Install::JsonHooks { files, create_if_missing, .. } => {
            let target = pick(files, *create_if_missing).ok_or("nothing to disconnect")?;
            let mut doc = read_json(&target)?;
            let n = hooks_remove(&mut doc);
            if n > 0 {
                write_json(&target, &doc)?;
            }
            Ok(format!("{n} hook entries removed from {}", target.display()))
        }
        Install::JsonMcp { files, servers_key, server_name, .. } => {
            let target = pick(files, false).ok_or("nothing to disconnect")?;
            let mut doc = read_json(&target)?;
            if mcp_remove(&mut doc, servers_key, server_name) {
                write_json(&target, &doc)?;
            }
            Ok(format!("The Blink MCP server was removed from {}", target.display()))
        }
        Install::File { path, .. } => {
            let target = expand(path);
            // Only a file that is Blink's own (it carries the marker) is deleted.
            if std::fs::read_to_string(&target).is_ok_and(|t| t.contains(MARKER)) {
                std::fs::remove_file(&target).map_err(|e| format!("cannot remove {}: {e}", target.display()))?;
            }
            Ok(format!("Removed {}", target.display()))
        }
        Install::Snippet { .. } => Err("this one is for copying".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("blink-connectors-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx() -> Ctx {
        Ctx::new(PathBuf::from(r"C:\Program Files\Blink\blinkctl.exe"), None)
    }

    fn events() -> Vec<(String, Option<String>)> {
        vec![("Stop".into(), None), ("Notification".into(), Some("*".into()))]
    }

    #[test]
    fn every_shipped_connector_parses_and_the_ids_are_unique() {
        let all = builtin();
        assert_eq!(all.len(), BUILTIN.len(), "one of the manifests does not parse");
        let mut ids: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        for c in &all {
            assert!(!c.name.is_empty() && !c.description.is_empty(), "{}", c.id);
        }
    }

    #[test]
    fn hooks_are_added_next_to_the_ones_already_there_and_removed_alone() {
        let mut doc = json!({ "model": "x", "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "echo mine" }] }], "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "lint.sh" }] }] } });
        let before = doc.clone();
        let command = ctx().fill("{{blinkctl}} hook claude");
        assert_eq!(command, "\"C:/Program Files/Blink/blinkctl.exe\" hook claude");
        hooks_add(&mut doc, &events(), &command, &Map::new()).unwrap();
        assert!(hooks_present(&doc));
        assert_eq!(doc["hooks"]["Stop"].as_array().unwrap().len(), 2, "next to the person's own");
        assert_eq!(doc["hooks"]["Notification"][0]["matcher"], "*");
        // Adding twice does not stack them.
        hooks_add(&mut doc, &events(), &command, &Map::new()).unwrap();
        assert_eq!(doc["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(hooks_remove(&mut doc), 2);
        assert_eq!(doc, before, "removing leaves the document as it was");
        assert!(!hooks_present(&doc));
        assert_eq!(hooks_remove(&mut doc), 0);
    }

    #[test]
    fn a_handler_can_carry_extra_fields_and_an_empty_document_works() {
        let mut doc = json!({});
        let mut handler = Map::new();
        handler.insert("name".into(), json!("Blink"));
        hooks_add(&mut doc, &events(), "\"x/blinkctl.exe\" hook gemini", &handler).unwrap();
        assert_eq!(doc["hooks"]["Stop"][0]["hooks"][0]["name"], "Blink");
        hooks_remove(&mut doc);
        assert_eq!(doc, json!({}), "even the empty hooks object goes");
        assert!(hooks_add(&mut json!([1]), &events(), "c", &Map::new()).is_err());
        assert!(hooks_add(&mut json!({ "hooks": { "Stop": 5 } }), &events(), "c", &Map::new()).is_err());
    }

    #[test]
    fn an_mcp_entry_is_added_and_removed_without_touching_the_others() {
        let mut doc = json!({ "mcpServers": { "other": { "command": "x" } } });
        mcp_add(&mut doc, "mcpServers", "blink", "C:/b/blinkctl.exe", &["mcp".to_string()]).unwrap();
        assert!(mcp_present(&doc, "mcpServers", "blink"));
        assert!(mcp_remove(&mut doc, "mcpServers", "blink"));
        assert_eq!(doc, json!({ "mcpServers": { "other": { "command": "x" } } }));
        assert!(!mcp_remove(&mut doc, "mcpServers", "blink"));
    }

    #[test]
    fn connecting_writes_one_backup_and_disconnecting_restores_the_content() {
        let dir = temp("hooks");
        let file = dir.join("settings.json");
        std::fs::write(&file, "{\n  \"theme\": \"dark\"\n}\n").unwrap();
        let manifest = format!(
            r#"{{ "id": "t", "name": "Test", "description": "d", "install": {{ "kind": "json-hooks", "files": [{}], "command": "{{{{blinkctl}}}} hook claude", "events": {{ "Stop": null }} }} }}"#,
            serde_json::to_string(&file.to_string_lossy()).unwrap()
        );
        let c = Connector::parse(&manifest, false).unwrap();
        assert_eq!(status(&c), Status { found: true, connected: false, target: Some(file.clone()) });
        connect(&c, &ctx()).unwrap();
        assert!(status(&c).connected);
        let backup = PathBuf::from(format!("{}.blink-backup", file.display()));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{\n  \"theme\": \"dark\"\n}\n", "the first state is kept");
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["theme"], "dark");
        connect(&c, &ctx()).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{\n  \"theme\": \"dark\"\n}\n", "a second connect does not replace the backup");
        disconnect(&c).unwrap();
        assert!(!status(&c).connected);
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc, json!({ "theme": "dark" }));
    }

    #[test]
    fn a_file_that_is_not_plain_json_is_left_alone_and_a_missing_agent_is_said() {
        let dir = temp("bad");
        let file = dir.join("settings.json");
        std::fs::write(&file, "{ // a comment\n}").unwrap();
        let manifest = format!(
            r#"{{ "id": "t", "name": "Test", "description": "d", "install": {{ "kind": "json-hooks", "files": [{}], "command": "{{{{blinkctl}}}} hook claude", "events": {{ "Stop": null }} }} }}"#,
            serde_json::to_string(&file.to_string_lossy()).unwrap()
        );
        let c = Connector::parse(&manifest, false).unwrap();
        let err = connect(&c, &ctx()).unwrap_err();
        assert!(err.contains("does not touch it"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ // a comment\n}");
        let missing = Connector::parse(&manifest.replace(&serde_json::to_string(&file.to_string_lossy()).unwrap(), "\"Z:/nowhere/at/all/settings.json\""), false).unwrap();
        assert!(connect(&missing, &ctx()).unwrap_err().contains("does not seem to be installed"));
        assert!(!status(&missing).found);
    }

    #[test]
    fn a_plugin_file_is_written_and_only_removed_when_it_is_blinks() {
        let dir = temp("plugin");
        let file = dir.join("plugins").join("blink.js");
        let manifest = format!(
            r#"{{ "id": "p", "name": "Plugin", "description": "d", "install": {{ "kind": "file", "path": {}, "content": "// {{{{marker}}}}\nconst ctl = '{{{{blinkctl_path}}}}'" }} }}"#,
            serde_json::to_string(&file.to_string_lossy()).unwrap()
        );
        let c = Connector::parse(&manifest, false).unwrap();
        connect(&c, &ctx()).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(MARKER) && text.contains("C:/Program Files/Blink/blinkctl.exe"));
        assert!(status(&c).connected);
        disconnect(&c).unwrap();
        assert!(!file.exists());
        // Someone else's file of the same name stays.
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "// mine").unwrap();
        disconnect(&c).unwrap();
        assert!(file.exists());
    }

    #[test]
    fn the_command_path_avoids_spaces_when_it_can() {
        assert_eq!(Ctx::new(PathBuf::from(r"C:\Users\me\AppData\Local\Programs\Blink\blinkctl.exe"), None).cmd, "C:/Users/me/AppData/Local/Programs/Blink/blinkctl.exe");
        assert_eq!(Ctx::new(PathBuf::from(r"C:\Program Files\Blink\blinkctl.exe"), Some(r"C:\PROGRA~1\Blink\blinkctl.exe".into())).cmd, "C:/PROGRA~1/Blink/blinkctl.exe");
        assert_eq!(Ctx::new(PathBuf::from(r"C:\Program Files\Blink\blinkctl.exe"), None).cmd, "\"C:/Program Files/Blink/blinkctl.exe\"");
    }

    #[test]
    fn manifests_are_checked() {
        assert!(Connector::parse("nope", false).is_err());
        assert!(Connector::parse(r#"{ "id": "has space", "name": "x", "install": { "kind": "snippet", "text": "t" } }"#, false).is_err());
        assert!(Connector::parse(r#"{ "id": "x", "name": "x", "install": { "kind": "mystery" } }"#, false).is_err());
        assert!(Connector::parse(r#"{ "id": "x", "name": "x", "install": { "kind": "json-hooks", "files": [], "command": "c", "events": { "Stop": null } } }"#, false).is_err());
        let c = Connector::parse(r#"{ "id": "x", "name": "x", "docs": "http://insecure", "install": { "kind": "snippet", "text": "run {{blinkctl}}" } }"#, false).unwrap();
        assert_eq!(c.docs, None, "only https links are kept");
        assert_eq!(c.snippet(&ctx()).as_deref(), Some("run \"C:/Program Files/Blink/blinkctl.exe\""));
        assert!(!c.installable());
    }

    #[test]
    fn the_person_s_own_manifests_are_read_and_can_replace_a_shipped_one() {
        let dir = temp("user");
        std::fs::write(dir.join("mine.json"), r#"{ "id": "my-agent", "name": "My agent", "install": { "kind": "snippet", "text": "x" } }"#).unwrap();
        std::fs::write(dir.join("claude.json"), r#"{ "id": "claude-code", "name": "Claude Code (mine)", "install": { "kind": "snippet", "text": "x" } }"#).unwrap();
        std::fs::write(dir.join("broken.json"), "{ nope").unwrap();
        let list = all(Some(&dir));
        assert!(list.iter().any(|c| c.id == "my-agent" && c.user));
        assert_eq!(list.iter().find(|c| c.id == "claude-code").map(|c| c.name.as_str()), Some("Claude Code (mine)"));
        assert_eq!(list.len(), builtin().len() + 1);
    }

    #[test]
    fn paths_expand_environment_variables_and_blink_home_stands_in_for_the_profile() {
        std::env::set_var("BLINK_TEST_VAR", "value");
        assert_eq!(expand("%BLINK_TEST_VAR%/x").to_string_lossy(), "value/x");
        assert_eq!(expand("%NO_SUCH_VARIABLE_ANYWHERE%/x").to_string_lossy(), "%NO_SUCH_VARIABLE_ANYWHERE%/x");
        assert_eq!(expand("100%").to_string_lossy(), "100%");
    }
}
