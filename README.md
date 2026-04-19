# opshub

**AI agent orchestrator for the terminal.** Spawn Claude Code, Codex, Kimi, and friends in parallel, capture every byte they emit into a searchable SQLite store, and manage them from a single tmux pane.

> Status: **v0.0.3 — cost parsers.** PTY runner + ratatui grid + live $ / tok/s header fed by Claude Code / Codex JSONL tailers. MCP server and hook bridge land in upcoming slices (see [ROADMAP](#roadmap)).

## Why another terminal-adjacent tool?

Not an emulator. opshub is **the layer on top of Ghostty / WezTerm / iTerm2 / tmux** that already-heavy AI-CLI users need.

|                                           | Chloe        | Wave Terminal   | Warp            | **opshub**                  |
| ----------------------------------------- | ------------ | --------------- | --------------- | --------------------------- |
| Scope                                     | AI-agent TUI | Full terminal   | Full terminal   | **tmux-layer orchestrator** |
| MCP **server** (other agents can call it) | ?            | ✕               | ✕               | **✓ (roadmap core)**        |
| Ingests Claude Code hooks                 | ✕            | ✕               | ✕               | **✓**                       |
| Lives inside tmux as a pane               | ?            | separate window | separate window | **✓**                       |
| Binary size                               | Rust         | Electron 200MB+ | cloud           | **Rust, <10MB**             |
| Cloud required                            | no           | no              | **yes**         | no                          |

## Design tenets

1. **MCP-native.** opshub is itself an MCP server. Any Claude Code / Codex instance you authorise can call `list_agents`, `search_history`, `dispatch` — your agents see their siblings.
2. **Ride the existing hook ecosystem.** If you already have `~/.claude/settings.json` full of `PostToolUse` hooks, add one line and every tool call shows up in opshub's timeline.
3. **tmux-first, GUI-later.** Runs as a pane inside your existing workflow. A Tauri GUI shell may come later as an optional front-end over the same `opshub-core` crate.

## Architecture (MVP)

```
┌── opshub process ──────────────────────────────┐
│ UI(ratatui) ←── event bus (tokio broadcast) ──→│
│                    ↑         ↓                  │
│  Agent Runner → PTY Host → Parsers(ANSI/cost)  │
│                              ↓                  │
│                        Storage (SQLite + FTS5) │
│                              ↓                  │
│                       MCP Server (rmcp)         │
│                              ↓                  │
│                    Unix socket (opshub CLI)    │
└────────────────────────────────────────────────┘
        ↑                       ↑
  claude / codex / kimi    ~/.claude hooks → emit
                           claude-peers MCP → subscribe
```

## Install

Nothing published yet. Build from source:

```sh
git clone https://github.com/rrrrnmtsu/opshub.git
cd opshub
cargo build --release

# drop the binary somewhere on PATH
install -m 0755 target/release/opshub ~/.local/bin/opshub

# install the bundled profiles so `opshub tui -a claude-code -a codex` works
mkdir -p ~/.config/opshub/agents
cp agents/claude-code.yaml agents/codex.yaml agents/_smoke.yaml ~/.config/opshub/agents/
```

### macOS .app bundle

For a Launchpad / Dock-native install, build a proper .app bundle and
drop it into `/Applications`:

```sh
./scripts/build-macos-app.sh
cp -R target/macos/opshub.app /Applications/
```

The bundle is a thin double-click shim — it opens Terminal.app and runs
`opshub tui -a claude-code -a codex`. First launch seeds
`~/.config/opshub/agents/` from the bundled profiles and symlinks
`~/.local/bin/opshub` so CLI usage keeps working.

Edit `/Applications/opshub.app/Contents/MacOS/opshub-launcher` if you
want a different default agent set. The bundle is unsigned; the first
launch may trigger Gatekeeper — right-click → **Open** once to approve.

Homebrew tap and prebuilt binaries arrive with v0.1.0.

## Quick start

```sh
# verify the install
opshub --version
opshub db-path
opshub agents        # list profiles found under ~/.config/opshub/agents

# one-off adhoc command — PTY output mirrors to your terminal AND streams
# into SQLite.
opshub launch --command "/bin/sh -c 'echo hello from opshub'"

# smoke test (prints two lines, exits 0)
opshub launch -a _smoke

# spin up N agents side-by-side in a ratatui grid.
# Tab / Shift-Tab cycles focus, Ctrl-M toggles the $ / tok/s header,
# Ctrl-Q quits. Agent names resolve against ~/.config/opshub/agents.
opshub tui -a claude-code -a codex

# search every session you ever ran (FTS5 over ANSI-stripped stdout)
opshub search "authentication bug"
```

Per-agent cost (`$`) and a 60-second `tok/s` window appear in the header as
soon as the upstream CLI writes its first usage record — Claude Code's
`~/.claude/projects/<cwd>/<session>.jsonl` or Codex's
`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Unknown models fall back to
`$0.00` with a log-only warning.

## Roadmap

MVP slices (v0.0.x → v0.1.0):

- [x] **v0.0.1**: workspace, schema + FTS5, PTY runner, echo E2E, `opshub launch|search|db-path`
- [x] **v0.0.2**: ratatui N×M pane grid, keyboard routing, resize (`opshub tui --profile a.yaml --profile b.yaml`)
- [x] **v0.0.3**: Claude Code + Codex cost parsers, live $ / tok/s header (toggle with `Ctrl-M`)
- [ ] **v0.0.4**: `emit-to-opshub.sh` drop-in hook + `opshub emit` subcommand, Unix socket
- [ ] **v0.0.5**: MCP server (`rmcp`): `list_agents`, `search_history`, `get_session_transcript`, `dispatch`, `get_cost_summary`
- [ ] **v0.0.6**: claude-peers MCP subscriber → `agent_message` timeline
- [ ] **v0.1.0**: docs polish, Homebrew tap, demo GIF, first announce

Phase 2 candidates: Tauri GUI shell, DuckDB analytics view, asciinema export, WASM plugins.

## License

Apache-2.0. See [LICENSE](./LICENSE).
