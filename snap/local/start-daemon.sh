#!/bin/bash
# Wrapper script to ensure config exists before starting the daemon

set -e

# Define paths relative to snap user directories
CONFIG_DIR="$HOME/.config/calendar-notifications-modal"
CONFIG_FILE="$CONFIG_DIR/config.toml"
DATA_DIR="$HOME/.local/share/calendar-notifications-modal"

# Create directories if they don't exist
mkdir -p "$CONFIG_DIR"
mkdir -p "$DATA_DIR"

# Generate default config if it doesn't exist
if [ ! -f "$CONFIG_FILE" ]; then
    echo "First run detected. Generating default configuration at $CONFIG_FILE"
    cat "$SNAP/etc/config-template.toml" > "$CONFIG_FILE"
    chmod 644 "$CONFIG_FILE"
    echo "Configuration created. Please edit $CONFIG_FILE to add your calendar backends."
    echo "After editing, restart the service with:"
    echo "  systemctl --user restart snap.calendar-notifications-modal.daemon.service"
fi

# Set XDG directories for the app
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"

# Start the daemon
exec "$SNAP/bin/calendar-notifications-modal" "$@"
