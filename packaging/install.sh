#!/usr/bin/env bash
# Install Calendar Notifications Modal as a systemd user service.
set -euo pipefail

BIN_NAME="calendar-notifications-modal"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo ">> Building release binary..."
( cd "$repo_root" && cargo build --release --bin "$BIN_NAME" )

echo ">> Installing binary to $BIN_DIR"
mkdir -p "$BIN_DIR"
install -m 0755 "$repo_root/target/release/$BIN_NAME" "$BIN_DIR/$BIN_NAME"

echo ">> Installing systemd user unit to $UNIT_DIR"
mkdir -p "$UNIT_DIR"
install -m 0644 "$repo_root/packaging/$BIN_NAME.service" "$UNIT_DIR/$BIN_NAME.service"

# Ensure the user manager has the graphical session environment (Wayland/X11,
# D-Bus) so the GUI can be displayed.
echo ">> Importing graphical-session environment"
if command -v systemctl >/dev/null 2>&1; then
  systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_CURRENT_DESKTOP 2>/dev/null || true
  systemctl --user daemon-reload
  systemctl --user enable --now "$BIN_NAME.service"
  echo ">> Service status:"
  systemctl --user --no-pager status "$BIN_NAME.service" || true
else
  echo "!! systemctl not found; binary installed but service not enabled."
fi

echo
echo "Done. Edit your config at:"
echo "  ${XDG_CONFIG_HOME:-$HOME/.config}/$BIN_NAME/config.toml"
echo "Then restart with: systemctl --user restart $BIN_NAME.service"
