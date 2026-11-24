#!/bin/sh
# Build and install After Dark for the current user (no root needed).
#
#   ./install.sh             build (release) and install into ~/.local
#   ./install.sh --uninstall remove it again (your AD_PREFS.INI etc. are kept)
#
# Installs:
#   ~/.local/bin/after-dark
#   ~/.local/share/applications/after-dark.desktop   (Control Panel launcher)
#   ~/.local/share/icons/hicolor/128x128/apps/after-dark.png
# and, unless --no-autostart is given, enables the idle daemon at login and starts it now.
set -eu

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/after-dark"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/128x128/apps"
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/after-dark-daemon.desktop"
cd "$(dirname "$0")"

if [ "${1:-}" = "--uninstall" ]; then
    pkill -x after-dark 2>/dev/null || true
    rm -f "$BIN" "$APPS/after-dark.desktop" "$ICONS/after-dark.png" "$AUTOSTART"
    echo "After Dark removed. Settings remain in ${XDG_CONFIG_HOME:-$HOME/.config}/after-dark/"
    exit 0
fi

cargo build --release -p ad-host
install -Dm755 target/release/after-dark "$BIN"
install -Dm644 assets/linux/after-dark.desktop "$APPS/after-dark.desktop"
install -Dm644 assets/linux/after-dark.png "$ICONS/after-dark.png"
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" 2>/dev/null || true

if [ "${1:-}" != "--no-autostart" ]; then
    mkdir -p "$(dirname "$AUTOSTART")"
    cat > "$AUTOSTART" <<DESKTOP
[Desktop Entry]
Type=Application
Name=After Dark
Comment=After Dark screen saver idle monitor
Exec="$BIN" daemon
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
DESKTOP
    "$BIN" daemon >/dev/null 2>&1 &
fi

echo "Installed $BIN"
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Note: $PREFIX/bin is not on your PATH." ;; esac
echo "Open \"After Dark\" from your applications menu, or run: after-dark"
