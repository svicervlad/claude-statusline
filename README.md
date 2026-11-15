# claude-statusline

One-row status line for Claude Code, in Rust.

```
Opus 5.5 high │ ctx ███░░ 63%/1M │ 5h 24% ↻16:00 │ 7d 87% ↻Вт 22:33 │ ~/proj  main* │ cache 91% │ 1h30m │ +120/-7
```

Segments, in priority order (the lowest drop first when the terminal is narrow):
model + effort, context usage, 5h / 7d plan limits with reset time, cost estimate (only without
plan limits), directory + git branch (`*` = uncommitted changes), prompt-cache hit ratio, session time, lines changed.
Percentages: green < 50, yellow < 80, red ≥ 80.

The two git calls run concurrently with a 500 ms timeout; untracked files are not scanned.
About 1.8 ms per run vs 12.9 ms for the equivalent bash + jq script.

## Install

```sh
cargo install --path .
```

`~/.claude/settings.json`:

```json
"statusLine": { "type": "command", "command": "~/.cargo/bin/claude-statusline" }
```

Input fields: <https://code.claude.com/docs/en/statusline#available-data>
