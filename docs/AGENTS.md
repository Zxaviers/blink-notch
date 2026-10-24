# AI agents on the notch

Any program can tell Blink what it is doing: an AI agent, a build, a long script. The notch shows it on the **Agents** page, peeks when one needs you or finishes, and shows one that is waiting for you on the resting pill.

Nothing is stored on disk and nothing leaves the PC: reports travel over the notch's private pipe, which only programs running as you can reach. Turn reports off in Blink settings, Agents.

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

## Claude Code

Put this in `~/.claude/settings.json` (or a project's `.claude/settings.json`), with the path to `blinkctl.exe` if it is not on your PATH. The event names are the ones Claude Code's hooks use; check your version's hooks page if one is not called as shown.

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "blinkctl agent --id claude-code --name \"Claude Code\" --state running --title \"Working\" --quiet" }] }],
    "Notification":     [{ "hooks": [{ "type": "command", "command": "blinkctl agent --id claude-code --state input --title \"Needs your attention\" --quiet" }] }],
    "Stop":             [{ "hooks": [{ "type": "command", "command": "blinkctl agent --id claude-code --state done --title \"Finished\" --quiet" }] }],
    "SessionEnd":       [{ "hooks": [{ "type": "command", "command": "blinkctl agent --id claude-code --remove --quiet" }] }]
  }
}
```

For two sessions at once, give each its own id (for example one per project folder).

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

No manifest or per-program permission step: any program running as you may report. No two-way messages (the notch cannot answer an agent's question yet); an agent that wants an answer should say so with `input` and wait for you in its own window.
