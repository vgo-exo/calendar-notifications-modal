#!/bin/bash
# Wrapper script for manual command-line usage

set -e

# Define paths
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
    echo "Configuration created at $CONFIG_FILE"
    echo ""
fi

# Set XDG directories for the app
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"

# Execute the app with any passed arguments (e.g., --login)
exec "$SNAP/bin/calendar-notifications-modal" "$@"
