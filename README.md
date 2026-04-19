# opshub

**AI agent orchestrator for the terminal.** Spawn Claude Code, Codex, Kimi, and friends in parallel, capture every byte they emit into a searchable SQLite store, and manage them from a single tmux pane.

> Status: **v0.0.1 — scaffold.** PTY runner, storage, and CLI skeleton only. TUI grid, MCP server, and cost parsers land in upcoming slices (see [ROADMAP](#roadmap)).

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
./target/release/opshub --help
```

Homebrew tap and prebuilt binaries arrive with v0.1.0.

## Quick start

```sh
# print where opshub will keep its database
opshub db-path

# launch an agent (adhoc command). PTY output mirrors to your terminal AND
# streams into SQLite.
opshub launch --command "/bin/sh -c 'echo hello from opshub'"

# launch a declared profile
opshub launch --profile agents/claude-code.yaml

# search every session you ever ran
opshub search "authentication bug"
```

## Roadmap

MVP slices (v0.0.x → v0.1.0):

- [x] **v0.0.1**: workspace, schema + FTS5, PTY runner, echo E2E, `opshub launch|search|db-path`
- [ ] **v0.0.2**: ratatui N×M pane grid, keyboard routing, resize
- [ ] **v0.0.3**: Claude Code + Codex cost parsers, live $ / tok/s header
- [ ] **v0.0.4**: `emit-to-opshub.sh` drop-in hook + `opshub emit` subcommand, Unix socket
- [ ] **v0.0.5**: MCP server (`rmcp`): `list_agents`, `search_history`, `get_session_transcript`, `dispatch`, `get_cost_summary`
- [ ] **v0.0.6**: claude-peers MCP subscriber → `agent_message` timeline
- [ ] **v0.1.0**: docs polish, Homebrew tap, demo GIF, first announce

Phase 2 candidates: Tauri GUI shell, DuckDB analytics view, asciinema export, WASM plugins.

## License

Apache-2.0. See [LICENSE](./LICENSE).
