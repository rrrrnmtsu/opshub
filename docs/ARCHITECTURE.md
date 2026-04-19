# opshub architecture

## Crates

| Crate           | Role                                                                                                                                                     |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `opshub-core`   | SQLite schema + migrations, `Event` / `EventKind` types, ANSI stripper, platform paths, `Storage` API. No async, no I/O beyond SQLite and `directories`. |
| `opshub-runner` | PTY lifecycle. Spawns a child via `portable-pty`, bridges its stdout bytes into a `tokio::sync::broadcast` and into `Storage`.                           |
| `opshub-cli`    | `opshub` binary. Thin CLI over core + runner. Today: `launch`, `search`, `db-path`. Future: `emit`, `attach`, `mcp`.                                     |

Later slices:

| Crate            | Role                                                                                                                        |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `opshub-parsers` | Claude Code / Codex / Kimi cost & tool-use extractors.                                                                      |
| `opshub-mcp`     | `rmcp`-based MCP server exposing `list_agents`, `search_history`, `get_session_transcript`, `dispatch`, `get_cost_summary`. |
| `opshub-tui`     | `ratatui` N×M grid, keyboard routing, full-screen interactive.                                                              |

## Event flow

```
child process
     │ PTY bytes
     ▼
+-----------------+
| blocking thread | (pty reader)
+--------+--------+
         │ std::mpsc
         ▼
+--------+----------------+
| spawn_blocking bridge   | ─► ansi::strip ─► Storage.insert_event(Stdout)
+--------+----------------+
         │
         ▼
+--------+----------------+
| tokio::sync::broadcast  | ─► TUI subscribers
+-------------------------+    CLI stdout mirror
                               (future) cost/parser pipeline
                               (future) MCP push notifications
```

## Why `portable-pty` + blocking reader

`portable-pty` exposes synchronous `Read`/`Write`. Wrapping it in tokio
primitives directly is brittle (file descriptors and `AsyncFd` differ by
platform). The idiomatic path is a dedicated OS thread for reads, a
`std::sync::mpsc` handoff, and a `tokio::task::spawn_blocking` bridge into the
async world. This keeps the runtime clean and makes cross-platform (Linux /
macOS) behaviour uniform.

## Why one SQLite connection behind a Mutex

Write volume for a single laptop user is tiny compared to SQLite-in-WAL
headroom (thousands of writes/sec). We keep one connection, synchronous, and
move _writes_ onto blocking tasks. If contention ever shows up, the upgrade
path is `r2d2` + multiple connections (readers) with the writer remaining
single-threaded — exactly how WAL wants it.

## MCP posture (target)

opshub is simultaneously an MCP **server** (tools for other agents to call)
and a **client** (subscribes to claude-peers for agent-to-agent traffic). The
server ships first because it is the piece no competitor offers.
