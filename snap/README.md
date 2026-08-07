# Snap Package for Calendar Notifications Modal

This directory contains the snap packaging configuration for easy 1-click installation.

## Features

The snap package provides:
- **Automatic installation** with a single command
- **Auto-generated configuration** on first run
- **Automatic startup** with your graphical session (on boot)
- **Sandboxed execution** with appropriate permissions

## Building the Snap

### Prerequisites

Install snapcraft:
```bash
sudo snap install snapcraft --classic
```

### Build

From the repository root:
```bash
snapcraft
```

This will create a `.snap` file (e.g., `calendar-notifications-modal_0.1.0_amd64.snap`).

### Clean Build

To do a clean build:
```bash
snapcraft clean
snapcraft
```

## Installing the Snap

### From Local Build

```bash
sudo snap install --dangerous calendar-notifications-modal_*.snap
```

Note: The `--dangerous` flag is needed for local snaps not from the Snap Store.

### From Snap Store (when published)

```bash
sudo snap install calendar-notifications-modal
```

## Using the Snap

### First Run

After installation, the daemon automatically:
1. Starts with your graphical session
2. Generates a default configuration at `~/.config/calendar-notifications-modal/config.toml`
3. Waits for you to configure your calendar backends

### Configuration

Edit the configuration file:
```bash
nano ~/.config/calendar-notifications-modal/config.toml
```

Add your calendar backends (see examples in the config file).

After editing, restart the service:
```bash
systemctl --user restart snap.calendar-notifications-modal.daemon.service
```

### OAuth Setup

For Microsoft Graph or Google Calendar, you need to authenticate once:

```bash
calendar-notifications-modal.calendar-notifications-modal --login outlook
# or
calendar-notifications-modal.calendar-notifications-modal --login gcal
```

### Managing the Service

Check status:
```bash
systemctl --user status snap.calendar-notifications-modal.daemon.service
```

View logs:
```bash
journalctl --user -u snap.calendar-notifications-modal.daemon.service -f
```

Restart:
```bash
systemctl --user restart snap.calendar-notifications-modal.daemon.service
```

Stop:
```bash
systemctl --user stop snap.calendar-notifications-modal.daemon.service
```

### Uninstalling

```bash
sudo snap remove calendar-notifications-modal
```

## Snap Permissions

The snap requests these permissions:
- `desktop` / `desktop-legacy` - GUI access
- `wayland` / `x11` - Display server access
- `network` / `network-bind` - Internet access for calendar syncing
- `home` - Access to your home directory for config and data
- `gsettings` - GNOME settings access

## Publishing to Snap Store

### Prerequisites

1. Create a Snap Store account at https://snapcraft.io/
2. Login:
   ```bash
   snapcraft login
   ```

### Register the Name

```bash
snapcraft register calendar-notifications-modal
```

### Upload

```bash
snapcraft upload --release=stable calendar-notifications-modal_*.snap
```

### Channels

Snap supports release channels:
- `stable` - Production-ready releases
- `candidate` - Release candidates
- `beta` - Beta testing
- `edge` - Latest development builds

Example release to edge:
```bash
snapcraft upload --release=edge calendar-notifications-modal_*.snap
```

## Architecture Support

The snap is configured to build for:
- `amd64` (x86_64)
- `arm64` (ARM 64-bit)
- `armhf` (ARM 32-bit)

Snapcraft will build for the host architecture by default. For multi-arch builds, use:
```bash
snapcraft remote-build
```

## Troubleshooting

### Daemon not starting

Check logs:
```bash
journalctl --user -u snap.calendar-notifications-modal.daemon.service -n 50
```

### Permission issues

The snap is confined with strict security. If you encounter permission errors:
1. Ensure the config is in `~/.config/calendar-notifications-modal/`
2. Check snap connections: `snap connections calendar-notifications-modal`
3. Manually connect interfaces if needed: `snap connect calendar-notifications-modal:home`

### GTK4 display issues

The snap uses the GNOME extension which should handle GTK4 dependencies. If you see display issues:
1. Ensure you're running a recent GNOME session
2. Check that the snap has access to wayland/x11: `snap connections calendar-notifications-modal`

## Development

For development with hot-reload:
```bash
snapcraft --debug
```

This drops you into a build environment shell where you can test changes.

## Files Included

- `snapcraft.yaml` - Main snap configuration
- `snap/local/config-template.toml` - Default configuration template
- `snap/local/start-daemon.sh` - Daemon startup wrapper
- `snap/local/calendar-notifications-modal-wrapper.sh` - CLI wrapper
- `snap/hooks/install` - Post-install hook
- `snap/hooks/configure` - Configuration hook
