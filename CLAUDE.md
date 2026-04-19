# opshub — AI agent orchestrator

**TL;DR**: Rust workspace on top of Ghostty/tmux that spawns Claude Code, Codex, Kimi, etc. under PTYs, persists every byte to SQLite (FTS5), and will expose itself as an MCP server so agents see their siblings. Not a terminal emulator.

This file is project-specific Claude Code instructions. The user's global rules (`~/.claude/CLAUDE.md`) still apply.

## Repo & remote

- Local: `/Users/remma/dev/opshub`
- GitHub: https://github.com/rrrrnmtsu/opshub (public, Apache-2.0)
- Initial design plan: `~/.claude/plans/jazzy-scribbling-anchor.md` (approved — do not rewrite the spec without going back to it)

## Workspace layout

| Crate           | Purpose                                                                                                                                                                                  |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `opshub-core`   | SQLite schema + FTS5 + migrations, `Event`/`EventKind`, ANSI stripper, `Storage`, platform paths                                                                                         |
| `opshub-runner` | PTY lifecycle via `portable-pty`, reader thread → std::mpsc → bridge thread → tokio broadcast + `Storage.insert_event`. Waiter orders child.wait → reader.join → bridge.join → `Exited`. |
| `opshub-tui`    | ratatui N×M grid, `LineBuffer`, crossterm key routing, per-pane PTY resize                                                                                                               |
| `opshub-cli`    | `opshub launch / search / db-path / tui`                                                                                                                                                 |

Future crates (see roadmap): `opshub-parsers`, `opshub-mcp`.

## Daily commands

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all          # CI enforces this
cargo clippy --workspace --all-targets -- -Dwarnings   # CI enforces this
./target/debug/opshub tui --profile agents/claude-code.yaml --profile agents/codex.yaml
```

CI runs fmt + clippy + test on ubuntu + macos (`.github/workflows/ci.yml`). Keep it green.

## Design rules

1. **opshub is not a terminal emulator.** If a feature belongs inside Ghostty/tmux/alacritty, reject it. We live on top, not underneath.
2. **MCP-native is the differentiator.** Every roadmap decision should ask "does this make opshub a better MCP server for the user's other agents?"
3. **Claude Code hook ecosystem first.** Users already have `~/.claude/settings.json` full of hooks; our `emit-to-opshub.sh` piggy-backs on that instead of demanding new instrumentation.
4. **One binary, <10MB, starts in <100ms.** The pitch vs Wave/Warp (Electron/cloud) is weightlessness. Reject heavy deps casually; if you need to pull something big, write a note in the PR explaining why the weight is earned.
5. **tmux pane compatibility trumps standalone UX.** If a feature breaks when opshub runs inside a tmux pane, fix it there first.

## Coding conventions

- Rust edition 2021, MSRV pinned via `workspace.package.rust-version`.
- No comments explaining the obvious. Comments are for _why_, _invariants_, or _surprises only_.
- Errors with `anyhow::Context`. Library boundaries may want `thiserror` later; don't preempt.
- Public structs get short doc comments explaining purpose + invariants, not mechanism.
- Tests live next to code (`mod tests`), not in `tests/`, unless the test itself needs `dev-dependencies` only.
- Async only where async earns its keep. PTY reads are synchronous; keep them on dedicated OS threads.

## Before shipping a commit

- `cargo fmt` ✓
- `cargo clippy --workspace --all-targets -- -Dwarnings` ✓
- `cargo test --workspace` ✓
- README roadmap checkbox updated if this commit closes a roadmap slice
- Commit body describes _why_, not just _what_. Co-Authored-By Claude line at the end.

## Roadmap pointer

Authoritative roadmap lives in `README.md` under `## Roadmap`. Update it when completing a slice. Short version as of v0.0.2 ship:

- [x] v0.0.1 scaffold — schema, runner, CLI
- [x] v0.0.2 ratatui grid — `opshub tui`, input routing, resize
- [ ] v0.0.3 cost parsers — live $ / tok/s in header for Claude Code + Codex
- [ ] v0.0.4 `opshub emit` + Unix socket, `emit-to-opshub.sh` hook wired up
- [ ] v0.0.5 MCP server (`rmcp`): `list_agents`, `search_history`, `get_session_transcript`, `dispatch`, `get_cost_summary`
- [ ] v0.0.6 claude-peers subscriber → `agent_message` timeline
- [ ] v0.1.0 polish + Homebrew tap + demo GIF

## What NOT to do here

- Don't add a VT emulator. `LineBuffer` is intentionally dumb; proper emulation waits until a paying use case forces the complexity.
- Don't build a GUI. Tauri shell is Phase 2 and only if the TUI is already loved.
- Don't write shell scripts for what belongs in the CLI binary. The `opshub` binary should be the one tool users need.
- Don't reach for async + locks if a single sync connection + a blocking thread covers it. SQLite WAL is happy with one writer.

## Known paper cuts (document before fixing)

- `opshub launch --command "..."` splits on whitespace (no shell quoting). Use a YAML profile for anything nontrivial.
- `opshub tui` requires a real tty (raw mode); it exits immediately when stdin/out are piped. Acceptable for v0.0.2.
- LineBuffer drops color; colors come with v0.0.3 parser work.
- `~/dev/ops-hub` (with hyphen) is the user's cross-project workspace, not this repo. Don't conflate the two paths.
