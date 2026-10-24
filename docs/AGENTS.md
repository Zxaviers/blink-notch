# AI agents on the notch

Any program can tell Blink what it is doing: an AI agent, a build, a long script. The notch shows it on the **Agents** page, peeks when one needs you or finishes, and shows one that is waiting for you on the resting pill.

Nothing is stored on disk and nothing leaves the PC: reports travel over the notch's private pipe, which only programs running as you can reach. Turn reports off in Blink settings, Agents.

## Connecting an agent

Open **Blink settings, Agents, Connect your agents**. Blink lists the agents it knows how to connect, says which it found on this PC, and connects or disconnects each with one button. Connect changes that agent's own settings only when you press it, keeps a backup of the file the first time (`settings.json.blink-backup`), leaves everything else in the file alone, and Disconnect removes only what Blink added.

There is no single way to make every agent report, so Blink has four, from exact to universal:

| Way | For | What the notch knows |
|---|---|---|
| **Hooks** (`blinkctl hook claude`, `... gemini`) | Agents with hooks: Claude Code, Gemini CLI | Exactly: working, waiting for you (with its own message), finished, failed; one entry per session, named for the project folder |
| **A plugin** | OpenCode (a small file in its plugin folder) | Working, needs your permission, finished, error |
| **MCP** (`blinkctl mcp`) | Any agent that speaks MCP, terminal or window: Cursor, Windsurf, Claude Desktop, Antigravity and more | What the agent chooses to report: its tool description asks it to say running, needs-you, done. Less exact, because the agent has to remember to call it |
| **Wrapper** (`blinkctl run -- command`) | Any program you start from a terminal | Running while it lives, then finished or failed by its exit code |

Blink only ships what it could check against the agent's documentation as "verified"; the others say so on their card. Where an agent keeps its settings can change between versions, so Blink writes only to files that exist and otherwise offers **Copy**: paste the entry where the agent says its MCP settings are.

### Adding an agent yourself

A connector is one small JSON file in `connectors` inside Blink's data folder (`%LOCALAPPDATA%\Blink\connectors\my-agent.json`, or where `BLINK_DATA_DIR` points); it appears in the list at once, and a file with a shipped id replaces the shipped one. The shipped ones are in the `connectors` folder of the source and are the best examples.

```json
{
  "id": "my-agent",
  "name": "My agent",
  "description": "What connecting does, in a sentence.",
  "docs": "https://example.com/docs",
  "detect": ["%USERPROFILE%\.my-agent"],
  "install": {
    "kind": "json-hooks",
    "files": ["%USERPROFILE%\.my-agent\settings.json"],
    "command": "{{blinkctl}} hook claude",
    "events": { "Stop": null, "Notification": null }
  }
}
```

`kind` is `json-hooks` (hook entries `event -> [{matcher, hooks:[{type:"command", command}]}]` in a JSON settings file; an optional `handler` object adds fields to each entry, `matcher` per event goes in `events`), `json-mcp` (an entry under `servers_key`, default `mcpServers`, named `server_name`, default `blink`, running `blinkctl mcp`), `file` (one file written whole, with `{{marker}}` somewhere in it so Blink only ever deletes its own) or `snippet` (text to copy; nothing is written). `files` may list several places: the first that exists is used. In paths, `%NAME%` is an environment variable; in commands and contents `{{blinkctl}}` is the path of `blinkctl.exe` as a shell command and `{{blinkctl_path}}` the plain path. A hook command that is not one of the two dialects can still use the plain `blinkctl agent ...` commands below.

New hook dialects (how another agent names its events) are a short table in `blink-core/src/agent_hooks.rs`.

## The command

`blinkctl.exe` sits next to `blink.exe` (for example `%LOCALAPPDATA%\Programs\Blink\blinkctl.exe`).

```
blinkctl agent --id claude-1 --name "Claude Code" --state running --title "Refactor the parser"
blinkctl agent --id claude-1 --progress 0.4 --detail "3 of 8 files"
blinkctl agent --id claude-1 --state input --title "Run the tests?"
blinkctl agent --id claude-1 --state done --title "Finished in 4 min" --url https://github.com/me/repo/pull/7
blinkctl agent --id claude-1 --remove
blinkctl agent --list
```

| Option | Meaning |
|---|---|
| `--id` | A short name that stays the same for one agent: letters, digits, `.`, `_`, `-` (up to 40). Required. |
| `--name` | What the page shows (up to 40 characters). Defaults to the id. |
| `--state` | `running`, `input` (waiting for you), `done`, `error` or `idle`. A new agent starts as `running`. |
| `--title`, `--detail` | One line each (90 and 120 characters). A field you leave out keeps what it had. |
| `--progress` | 0 to 1: a thin line along the row. Reset when the state changes. |
| `--url` | An https link; a click on the row opens it. |
| `--quiet` | Exit 0 and say nothing when Blink is not running (for hooks that must never fail). |

Exit codes: 0 done, 2 wrong use, 3 Blink is not running, 4 Blink refused (the reason is printed, for example `BAD_ID`, `BAD_URL`, `AGENTS_OFF`).

### What the notch does

- `running`: listed, no peek.
- `input` (waiting for you) and `error`: a peek that stays longer, and the resting pill shows the agent's name and "need you" until it moves on. This is how you notice an agent that stopped to ask something while you were in another window.
- `done`: a short peek.
- The list shows the ones that want you first, then running, then the rest. An agent that has said nothing for 6 hours is forgotten (a crashed program never says it is done). Up to 12 agents are kept.

## Claude Code by hand

Connect does this for you. If you prefer to write it yourself, put this in `~/.claude/settings.json` (a project's `.claude/settings.json` works too); `blinkctl hook claude` reads the event and the session from what Claude Code gives the hook, so the same command serves every event and two sessions show as two agents:

```json
{
  "hooks": {
    "SessionStart":      [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }],
    "UserPromptSubmit":  [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }],
    "PostToolUse":       [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }],
    "Notification":      [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }],
    "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }],
    "Stop":              [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }],
    "StopFailure":       [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }],
    "SessionEnd":        [{ "hooks": [{ "type": "command", "command": "blinkctl hook claude" }] }]
  }
}
```

Use the full path to `blinkctl.exe` if it is not on your PATH (a path without spaces, with forward slashes, works in every shell).

## Any other tool

A shell script:

```powershell
blinkctl agent --id nightly --name "Nightly build" --state running --title "npm test" --quiet
npm test
if ($LASTEXITCODE -eq 0) { blinkctl agent --id nightly --state done --title "All green" --quiet }
else { blinkctl agent --id nightly --state error --title "Tests failed" --quiet }
```

Many updates from one program: `blinkctl agent --json` reads one JSON object per line from standard input (`{"id":"a","state":"running"}`) and forwards each.

## The pipe itself

Without the command: open `\\.\pipe\blink-ipc-main`, write one JSON line, read one JSON line back.

```json
{"cmd":"agent_update","id":"claude-1","name":"Claude Code","state":"input","title":"Run the tests?"}
{"cmd":"agent_remove","id":"claude-1"}
{"cmd":"agent_list"}
```

The answer is `{"ok":true,...}` or `{"ok":false,"error":"BAD_ID"}`. A second pipe name can be chosen with the `BLINK_IPC` environment variable (for tests).

## What is not here yet

No per-program permission step: any program running as you may report. No two-way messages (the notch cannot answer an agent's question yet); an agent that wants an answer should say so with `input` and wait for you in its own window.
