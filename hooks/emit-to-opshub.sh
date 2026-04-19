#!/usr/bin/env bash
# opshub drop-in hook for Claude Code `PostToolUse` / `Stop` events.
#
# Usage (snippet for ~/.claude/settings.json):
#   {
#     "hooks": {
#       "PostToolUse": [
#         { "matcher": ".*", "hooks": [{ "type": "command", "command": "~/.local/bin/opshub-emit.sh" }] }
#       ]
#     }
#   }
#
# The hook receives Claude Code's JSON payload on stdin. It forwards it to
# `opshub emit`, which inserts an `event(kind='hook')` row keyed on the
# session id provided via the CLAUDE_SESSION_ID env var (or generates one).
#
# NOTE: `opshub emit` is a post-v0.0.1 subcommand; for now this script just
# noops unless the binary exists, so it is safe to install ahead of time.

set -euo pipefail

if ! command -v opshub >/dev/null 2>&1; then
  exit 0
fi

# opshub emit is a no-op on MVP; future versions consume stdin as JSON.
exec opshub emit \
  --session "${CLAUDE_SESSION_ID:-unknown}" \
  --kind hook \
  --from claude-code
