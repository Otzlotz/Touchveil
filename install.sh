#!/bin/sh
# Builds Touchveil, installs it for the current user and sets up the permissions.
# The daemon then runs as a systemd user service; the overlay is started as an app
# (application menu, icon in the tray).
#
#   ./install.sh              install or update
#   ./install.sh --uninstall  remove again (the config is kept)
#
# PREFIX (default ~/.local) sets where the programs go.
set -eu

cd "$(dirname "$0")"
PREFIX=${PREFIX:-$HOME/.local}
BIN=$PREFIX/bin
CONFIG=${XDG_CONFIG_HOME:-$HOME/.config}/touchveil/config.toml
RULE=/etc/udev/rules.d/70-touchveil.rules
MODULES=/etc/modules-load.d/touchveil.conf
SERVICE=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/touchveil-daemon.service
DESKTOP=${XDG_DATA_HOME:-$HOME/.local/share}/applications/touchveil.desktop

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }

if [ "${1:-}" = "--uninstall" ]; then
    say "Stopping and removing the service"
    systemctl --user disable --now touchveil-daemon.service 2>/dev/null || true
    rm -f "$SERVICE" "$DESKTOP"
    systemctl --user daemon-reload 2>/dev/null || true
    say "Removing programs from $BIN"
    rm -f "$BIN/touchveil-daemon" "$BIN/touchveil-overlay"
    say "Removing udev rule (sudo)"
    sudo rm -f "$RULE" "$MODULES"
    sudo udevadm control --reload
    say "Done. The config at $CONFIG is kept."
    exit 0
fi

case $BIN in
    *[[:space:]]*) echo "PREFIX must not contain spaces: $BIN" >&2; exit 1 ;;
esac
command -v cargo >/dev/null || { echo "cargo is missing: install Rust (e.g. pacman -S rust)" >&2; exit 1; }
for lib in xkbcommon fontconfig wayland-client egl; do
    pkg-config --exists "$lib" 2>/dev/null || {
        echo "Library $lib is missing. Arch: pacman -S libxkbcommon fontconfig wayland mesa" >&2
        exit 1
    }
done

say "Building (takes a few minutes the first time)"
cargo build --release --locked

say "Installing to $BIN"
install -Dm755 target/release/touchveil-daemon "$BIN/touchveil-daemon"
install -Dm755 target/release/touchveil-overlay "$BIN/touchveil-overlay"

if [ -e "$CONFIG" ]; then
    say "Config exists, left unchanged: $CONFIG"
else
    say "Creating example config: $CONFIG"
    install -Dm644 config.example.toml "$CONFIG"
fi

say "Setting up access to the touchscreen and uinput (sudo)"
sudo install -Dm644 packaging/70-touchveil.rules "$RULE"
echo uinput | sudo tee "$MODULES" >/dev/null
sudo modprobe uinput
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=input --subsystem-match=misc

say "Creating app entry: $DESKTOP"
mkdir -p "$(dirname "$DESKTOP")"
sed "s|@BIN@|$BIN|g" packaging/touchveil.desktop > "$DESKTOP"

say "Setting up the touchveil-daemon service"
mkdir -p "$(dirname "$SERVICE")"
sed "s|@BIN@|$BIN|g" packaging/touchveil-daemon.service > "$SERVICE"
if systemctl --user daemon-reload 2>/dev/null; then
    systemctl --user enable touchveil-daemon.service
    # restart instead of start: picks up the new version on an update.
    systemctl --user restart touchveil-daemon.service
else
    echo "No systemd user service manager reachable (e.g. via sudo or ssh without a session)." >&2
    echo "After logging in, run: systemctl --user enable --now touchveil-daemon" >&2
fi

say "Done."
echo "Start Touchveil from the application menu or:  $BIN/touchveil-overlay &"
echo "To start it automatically with Hyprland (optional), in ~/.config/hypr/hyprland.conf:"
echo "    exec-once = $BIN/touchveil-overlay"
