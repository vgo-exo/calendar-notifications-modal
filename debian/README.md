# Debian Package for Calendar Notifications Modal

This directory contains Debian packaging configuration for creating native `.deb` packages.

## What's Here

- `control` - Package metadata, dependencies, and description
- `copyright` - License information
- `changelog` - Version history
- `compat` - Debhelper compatibility level
- `postinst` - Post-installation script
- `prerm` - Pre-removal script
- `postrm` - Post-removal script

## Building the Package

### Prerequisites

Install cargo-deb:
```bash
cargo install cargo-deb
```

### Build

From the repository root:
```bash
# Using the build script
./packaging/build-deb.sh

# Or using Make
make build-deb

# Or using cargo-deb directly
cargo build --release --bin calendar-notifications-modal
cargo deb --no-build -p cnm-app
```

This creates a `.deb` file at `target/debian/calendar-notifications-modal_*.deb`.

## Installing Locally

```bash
# Using Make (easiest)
make install-deb

# Or manually
sudo dpkg -i calendar-notifications-modal_*.deb
sudo apt --fix-broken install  # If dependencies are missing
```

## Package Contents

The package installs:
- `/usr/bin/calendar-notifications-modal` - Main binary
- `/usr/lib/systemd/user/calendar-notifications-modal.service` - Systemd user service
- `/usr/share/calendar-notifications-modal/config-template.toml` - Default config template
- `/usr/share/doc/calendar-notifications-modal/` - Documentation

## Dependencies

Runtime dependencies (automatically installed):
- `libgtk-4-1` - GTK4 runtime libraries
- `libglib2.0-0` - GLib runtime
- `systemd` - For systemd user services
- `xdg-utils` - For opening meeting links

Build dependencies (needed only for building):
- `cargo` and `rustc` - Rust toolchain
- `libgtk-4-dev` - GTK4 development libraries
- `pkg-config` - Build configuration tool

## Post-Installation

After installing, users need to:

1. Enable and start the service:
   ```bash
   systemctl --user enable calendar-notifications-modal.service
   systemctl --user start calendar-notifications-modal.service
   ```

2. Configure calendars by editing:
   ```bash
   ~/.config/calendar-notifications-modal/config.toml
   ```
   (Created automatically on first run with default template)

3. For OAuth setup:
   ```bash
   calendar-notifications-modal --login outlook
   # or
   calendar-notifications-modal --login gcal
   ```

## Sharing with Coworkers

Simply share the `.deb` file! Recipients can install with:
```bash
sudo dpkg -i calendar-notifications-modal_*.deb
sudo apt --fix-broken install  # Handles any missing dependencies
```

The package is typically 5-10MB, making it email/Slack friendly.

## Uninstalling

```bash
# Remove package but keep config
sudo apt remove calendar-notifications-modal

# Remove package AND config
sudo apt purge calendar-notifications-modal
```

User configuration remains at:
- `~/.config/calendar-notifications-modal/`
- `~/.local/share/calendar-notifications-modal/`

Remove manually if desired after purge.

## Version Management

To release a new version:

1. Update version in `Cargo.toml` (workspace level)
2. Update `debian/changelog`:
   ```bash
   dch -v 0.2.0-1 "New release with feature X"
   ```
3. Rebuild the package
4. Tag the release in git:
   ```bash
   git tag -a v0.2.0 -m "Release v0.2.0"
   git push --tags
   ```

## Package Validation

Check package contents:
```bash
dpkg -c calendar-notifications-modal_*.deb
```

Check package info:
```bash
dpkg -I calendar-notifications-modal_*.deb
```

Lint the package:
```bash
lintian calendar-notifications-modal_*.deb
```

## Troubleshooting

### Missing dependencies during install
```bash
sudo apt --fix-broken install
```

### GTK4 not available
The package requires GTK4, available in:
- Ubuntu 22.04 LTS and newer
- Debian 12 (Bookworm) and newer

For older systems, consider using the snap package instead.

### Service not starting
```bash
# Check status
systemctl --user status calendar-notifications-modal.service

# View logs
journalctl --user -u calendar-notifications-modal.service -n 50

# Check config
cat ~/.config/calendar-notifications-modal/config.toml
```

## Advanced: Using with a PPA

For easier distribution and updates, consider creating a Personal Package Archive:

1. Create a Launchpad account
2. Set up a PPA
3. Upload source package with:
   ```bash
   debuild -S
   dput ppa:yourusername/yourppa ../calendar-notifications-modal_*.changes
   ```

Users can then install with:
```bash
sudo add-apt-repository ppa:yourusername/yourppa
sudo apt update
sudo apt install calendar-notifications-modal
```

## Technical Notes

- Uses `cargo-deb` for simplified Rust packaging
- Systemd user service (not system-wide daemon)
- XDG-compliant config and data directories
- Maintainer scripts handle service management
- No debconf prompts for simplicity
- Compatible with Ubuntu 22.04+ and Debian 12+
