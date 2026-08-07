# Quick Start: Installing via Debian Package (.deb)

The best way to install Calendar Notifications Modal on Ubuntu/Debian systems is via the native Debian package.

## Why .deb Package?

✅ **Small file size** - 5-10MB (vs snap's 100-200MB)  
✅ **Fast installation** - Native package manager  
✅ **No snapd required** - Works on any Debian/Ubuntu  
✅ **Email-friendly** - Easy to share with coworkers  
✅ **Enterprise-friendly** - Traditional package format  

## Prerequisites

- Ubuntu 22.04 LTS or newer (or Debian 12+)
- GTK4 libraries (usually pre-installed on GNOME systems)

## 1. Build the Package

```bash
# Install cargo-deb (one-time)
cargo install cargo-deb

# Build the package
cd /path/to/calendar-notifications-modal
make build-deb

# Or use the script directly
./packaging/build-deb.sh
```

This creates `calendar-notifications-modal_0.1.0_amd64.deb` (or similar).

## 2. Install the Package

```bash
# Easy way (handles dependencies automatically)
make install-deb

# Or manually
sudo dpkg -i calendar-notifications-modal_*.deb
sudo apt --fix-broken install  # If dependencies are missing
```

## 3. Enable and Start the Service

```bash
systemctl --user enable calendar-notifications-modal.service
systemctl --user start calendar-notifications-modal.service
```

The service will now start automatically on login!

## 4. Configure Your Calendars

A default configuration is created at `~/.config/calendar-notifications-modal/config.toml` on first run.

Edit it to add your calendars:

```bash
nano ~/.config/calendar-notifications-modal/config.toml
```

### Example: Add an ICS calendar

```toml
[[backends]]
type = "ics"
id = "work"
url = "https://example.com/calendar.ics"
```

### Example: Add Microsoft 365 / Outlook

```toml
[[backends]]
type = "msgraph"
id = "outlook"
client_id = "your-azure-app-id"
tenant = "common"
```

Then authenticate:
```bash
calendar-notifications-modal --login outlook
```

### Example: Add Google Calendar

```toml
[[backends]]
type = "google"
id = "gcal"
client_id = "your-google-client-id.apps.googleusercontent.com"
client_secret = "your-google-client-secret"
calendar_id = "primary"
```

Then authenticate:
```bash
calendar-notifications-modal --login gcal
```

## 5. Restart the Service

After editing the config:

```bash
systemctl --user restart calendar-notifications-modal.service
```

## That's It! 🎉

The app will now:
- ✅ Start automatically on boot
- ✅ Show calendar reminder modals when events are due
- ✅ Restart automatically on failure

## Managing the Service

**Check status:**
```bash
systemctl --user status calendar-notifications-modal.service
```

**View logs:**
```bash
journalctl --user -u calendar-notifications-modal.service -f
```

**Stop:**
```bash
systemctl --user stop calendar-notifications-modal.service
```

**Restart:**
```bash
systemctl --user restart calendar-notifications-modal.service
```

**Disable auto-start:**
```bash
systemctl --user disable calendar-notifications-modal.service
```

## Sharing with Coworkers

To share the package with coworkers:

1. **Build the package** (as above)
2. **Share the .deb file** via email, Slack, shared drive, etc.
3. **They install** with:
   ```bash
   sudo dpkg -i calendar-notifications-modal_0.1.0_amd64.deb
   sudo apt --fix-broken install
   systemctl --user enable --now calendar-notifications-modal.service
   ```

The .deb file is self-contained and email-friendly (~5-10MB).

## Updating

To update to a new version:

1. Build the new version
2. Install it (dpkg will upgrade):
   ```bash
   sudo dpkg -i calendar-notifications-modal_0.2.0_amd64.deb
   ```
3. Restart the service:
   ```bash
   systemctl --user restart calendar-notifications-modal.service
   ```

## Uninstalling

**Remove but keep config:**
```bash
sudo apt remove calendar-notifications-modal
```

**Remove everything:**
```bash
sudo apt purge calendar-notifications-modal
rm -rf ~/.config/calendar-notifications-modal
rm -rf ~/.local/share/calendar-notifications-modal
```

## Troubleshooting

### Package won't install - missing dependencies

```bash
sudo apt update
sudo apt --fix-broken install
```

### GTK4 not available

The package requires GTK4, available in Ubuntu 22.04+ and Debian 12+.

For older systems:
- Use the snap package instead: `snap install calendar-notifications-modal`
- Or manually install from source

### Service won't start

```bash
# Check service status
systemctl --user status calendar-notifications-modal.service

# View logs
journalctl --user -u calendar-notifications-modal.service -n 50

# Check config syntax
calendar-notifications-modal --help
```

### Config file not created

The config is created on first run of the daemon. If it's not there:

```bash
# Manually copy the template
sudo cp /usr/share/calendar-notifications-modal/config-template.toml \
       ~/.config/calendar-notifications-modal/config.toml
```

### Modal window not appearing

Ensure you're running a graphical session (GNOME/Wayland recommended):

```bash
echo $XDG_SESSION_TYPE  # Should be "wayland" or "x11"
echo $WAYLAND_DISPLAY   # Should be set
```

For Wayland issues, check:
```bash
systemctl --user import-environment WAYLAND_DISPLAY XDG_RUNTIME_DIR
systemctl --user restart calendar-notifications-modal.service
```

## Comparison: .deb vs Snap

| Feature | .deb Package | Snap Package |
|---------|--------------|--------------|
| File size | ~5-10MB | ~100-200MB |
| Install speed | Fast | Slower |
| Startup time | Fast | Slightly slower |
| Auto-updates | No (manual) | Yes (if from Snap Store) |
| Sandboxing | No | Yes (confined) |
| Works on | Debian/Ubuntu | Many distros |
| Permissions | Normal | Snap interfaces |
| Dependencies | Uses system libs | Self-contained |

**Recommendation:**
- **For coworkers on Ubuntu/Debian**: Use .deb package (this guide)
- **For wider distribution**: Use snap package
- **For other distros**: Use snap package or build from source

## Building for Different Architectures

To build for different architectures:

```bash
# For ARM64
cargo build --release --target aarch64-unknown-linux-gnu
cargo deb --no-build -p cnm-app --target aarch64-unknown-linux-gnu

# For ARMv7
cargo build --release --target armv7-unknown-linux-gnueabihf
cargo deb --no-build -p cnm-app --target armv7-unknown-linux-gnueabihf
```

Note: Cross-compilation requires appropriate toolchains and system libraries.

## Next Steps

- Configure your calendars in the config file
- Set up OAuth for Microsoft Graph or Google Calendar
- Customize snooze intervals and reminder times
- Check out the main README for advanced features

Happy calendaring! 📅✨
