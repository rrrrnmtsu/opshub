#!/usr/bin/env bash
# Build a macOS .app bundle for opshub.
#
# Outputs:   target/macos/opshub.app
# Install:   cp -R target/macos/opshub.app /Applications/
#
# The .app is a thin double-click shim: its CFBundleExecutable is a shell
# launcher that opens Terminal.app and runs `opshub tui -a ... -a ...`.
# Power users who live in Ghostty / WezTerm / tmux do not need the bundle —
# `~/.local/bin/opshub` is the same binary, just wrapped.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

APP_DIR="target/macos/opshub.app"
MACOS_DIR="$APP_DIR/Contents/MacOS"
RESOURCES_DIR="$APP_DIR/Contents/Resources"

# Default agent names the launcher will fire. Override by editing the
# generated launcher script after install — plain shell, no config format.
DEFAULT_AGENTS=(claude-code codex)

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
: "${VERSION:=0.0.0}"

echo "==> building release binary"
cargo build --release -p opshub-cli

echo "==> assembling $APP_DIR"
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR/agents"

# Ship the same binary users would have on PATH; nothing bundle-specific.
cp target/release/opshub "$MACOS_DIR/opshub"
chmod +x "$MACOS_DIR/opshub"

# Bundled agent profiles — first-run populates ~/.config/opshub/agents so
# the CLI's -a flag keeps working after a pure .app install.
cp agents/claude-code.yaml agents/codex.yaml agents/_smoke.yaml \
   "$RESOURCES_DIR/agents/"

# Build the launcher script. Quoting rules inside `do script` are finicky;
# we pass the binary path literally and rely on the user not keeping spaces
# in the install path (which is /Applications for the common case).
AGENT_FLAGS=""
for a in "${DEFAULT_AGENTS[@]}"; do
    AGENT_FLAGS="$AGENT_FLAGS -a $a"
done

cat > "$MACOS_DIR/opshub-launcher" <<'LAUNCHER'
#!/usr/bin/env bash
# opshub.app entry point. Opens the user's preferred terminal emulator and
# runs `opshub tui ...`. We prefer Ghostty (the terminal most of our users
# actually live in) and fall back to Apple's Terminal.app so the bundle
# works on a stock macOS install too.
#
# Override the default with OPSHUB_LAUNCHER_TERM=<ghostty|terminal|iterm>.
# Override the default agent list by editing the CMD line at the bottom.
set -euo pipefail

BUNDLE="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="$BUNDLE/Contents/MacOS/opshub"

# Seed ~/.config/opshub/agents from the bundle on first run so the -a name
# resolution inside the binary works even without a separate install step.
CFG="$HOME/.config/opshub/agents"
if [ ! -d "$CFG" ]; then
    mkdir -p "$CFG"
    cp "$BUNDLE/Contents/Resources/agents/"*.yaml "$CFG/" 2>/dev/null || true
fi

# Mirror the binary into ~/.local/bin on first run so users who live in a
# terminal still get `opshub` on PATH without a second install step. A
# symlink is friendlier than a copy (auto-updates when the .app updates),
# but /Applications and ~/.local/bin may span filesystems so we try symlink
# first and fall back to a copy.
LOCAL_BIN="$HOME/.local/bin"
mkdir -p "$LOCAL_BIN"
if [ ! -e "$LOCAL_BIN/opshub" ]; then
    ln -s "$BIN" "$LOCAL_BIN/opshub" 2>/dev/null || cp "$BIN" "$LOCAL_BIN/opshub"
fi

CMD="'$BIN' tui __AGENT_FLAGS__"

# Detect the preferred terminal. Env var wins; otherwise we auto-detect by
# checking which apps exist under /Applications, in priority order.
choose_term() {
    if [ -n "${OPSHUB_LAUNCHER_TERM:-}" ]; then
        echo "$OPSHUB_LAUNCHER_TERM"
        return
    fi
    if [ -d "/Applications/Ghostty.app" ]; then
        echo ghostty
    elif [ -d "/Applications/WezTerm.app" ]; then
        echo wezterm
    elif [ -d "/Applications/iTerm.app" ]; then
        echo iterm
    else
        echo terminal
    fi
}

TERM_CHOICE="$(choose_term)"

case "$TERM_CHOICE" in
    ghostty)
        # Ghostty on macOS requires `open -na` because a bare `ghostty -e`
        # isn't a supported invocation on the Mac build.
        exec /usr/bin/open -na Ghostty.app --args -e "$CMD"
        ;;
    wezterm)
        exec /usr/bin/open -na WezTerm.app --args start -- sh -c "$CMD"
        ;;
    iterm)
        /usr/bin/osascript <<APPLESCRIPT
tell application "iTerm"
    activate
    create window with default profile command "$CMD"
end tell
APPLESCRIPT
        ;;
    terminal|*)
        /usr/bin/osascript <<APPLESCRIPT
tell application "Terminal"
    activate
    do script "$CMD"
end tell
APPLESCRIPT
        ;;
esac
LAUNCHER
# shellcheck disable=SC2016
sed -i '' "s| __AGENT_FLAGS__| ${AGENT_FLAGS# }|" "$MACOS_DIR/opshub-launcher"
chmod +x "$MACOS_DIR/opshub-launcher"

# Info.plist. Kept minimal — no icon, no UTI claim, no background hints.
# Add fields opportunistically as users report needing them (notarisation
# / first-run Gatekeeper behavior is out of scope for a local dev build).
cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>opshub</string>
    <key>CFBundleDisplayName</key><string>opshub</string>
    <key>CFBundleIdentifier</key><string>com.opshub.opshub</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleExecutable</key><string>opshub-launcher</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Strip the Gatekeeper quarantine bit set by Finder when the .app is
# downloaded or copied from a DMG. Local dev builds don't have it in the
# first place; this is defensive for when the user scripts a curl install.
xattr -rd com.apple.quarantine "$APP_DIR" 2>/dev/null || true

echo "==> built $APP_DIR (version $VERSION)"
echo "    install:    cp -R $APP_DIR /Applications/"
echo "    launcher:   $MACOS_DIR/opshub-launcher"
echo "    binary:     $MACOS_DIR/opshub"
