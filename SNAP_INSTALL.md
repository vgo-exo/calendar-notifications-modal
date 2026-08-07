# Quick Start: Installing via Snap

The easiest way to install Calendar Notifications Modal is via snap.

## 1. Build the Snap

```bash
# Install snapcraft if you haven't already
sudo snap install snapcraft --classic

# Build the snap from the repository root
cd /path/to/calendar-notifications-modal
snapcraft
```

This creates a `.snap` file (e.g., `calendar-notifications-modal_0.1.0_amd64.snap`).

## 2. Install the Snap

```bash
sudo snap install --dangerous calendar-notifications-modal_*.snap
```

The `--dangerous` flag is needed for local snaps not from the Snap Store.

## 3. That's It! 🎉

The snap package automatically:
- ✅ Generates a default config at `~/.config/calendar-notifications-modal/config.toml`
- ✅ Starts the daemon with your graphical session (on boot)
- ✅ Restarts on failure

## 4. Configure Your Calendars

Edit the auto-generated config:
```bash
nano ~/.config/calendar-notifications-modal/config.toml
```

Add your calendar backends. Examples are included in the config file.

Then restart:
```bash
systemctl --user restart snap.calendar-notifications-modal.daemon.service
```

## 5. OAuth Setup (if needed)

For Microsoft Graph or Google Calendar:
```bash
calendar-notifications-modal.calendar-notifications-modal --login outlook
# or
calendar-notifications-modal.calendar-notifications-modal --login gcal
```

## Managing the Service

**Check status:**
```bash
systemctl --user status snap.calendar-notifications-modal.daemon.service
```

**View logs:**
```bash
journalctl --user -u snap.calendar-notifications-modal.daemon.service -f
```

**Restart:**
```bash
systemctl --user restart snap.calendar-notifications-modal.daemon.service
```

## Uninstall

```bash
sudo snap remove calendar-notifications-modal
```

This removes the snap but keeps your config and data files in `~/.config/` and `~/.local/share/`.

## Publishing to Snap Store

Once you're ready to publish to the Snap Store:

```bash
snapcraft login
snapcraft register calendar-notifications-modal
snapcraft upload --release=stable calendar-notifications-modal_*.snap
```

Then users can install with:
```bash
sudo snap install calendar-notifications-modal
```

No `--dangerous` flag needed!
