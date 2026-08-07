# Package Validation Checklist

This document outlines the validation steps to verify the Debian package is properly built and functional.

## Pre-Build Validation

- [ ] Verify all debian/ files exist and are properly formatted
- [ ] Check Cargo.toml has correct [package.metadata.deb] section
- [ ] Verify LICENSE file exists in repository root
- [ ] Check maintainer scripts (postinst, prerm, postrm) are executable

```bash
ls -la debian/
chmod +x debian/postinst debian/prerm debian/postrm
```

## Build Validation

- [ ] Install cargo-deb if not present: `cargo install cargo-deb`
- [ ] Build release binary: `cargo build --release --bin calendar-notifications-modal`
- [ ] Build Debian package: `make build-deb` or `./packaging/build-deb.sh`
- [ ] Verify .deb file was created: `ls -lh calendar-notifications-modal_*.deb`

## Package Structure Validation

Check package contents:
```bash
dpkg -c calendar-notifications-modal_*.deb
```

Verify the following files are included:
- [ ] `/usr/bin/calendar-notifications-modal` (755)
- [ ] `/usr/lib/systemd/user/calendar-notifications-modal.service` (644)
- [ ] `/usr/share/calendar-notifications-modal/config-template.toml` (644)
- [ ] `/usr/share/doc/calendar-notifications-modal/README.md`
- [ ] `/usr/share/doc/calendar-notifications-modal/examples/config.example.toml`

## Package Metadata Validation

Check package information:
```bash
dpkg -I calendar-notifications-modal_*.deb
```

Verify:
- [ ] Package name is `calendar-notifications-modal`
- [ ] Version matches Cargo.toml (e.g., 0.1.0)
- [ ] Architecture is appropriate (amd64, arm64, etc.)
- [ ] Section is `utils`
- [ ] Priority is `optional`
- [ ] Dependencies include: libgtk-4-1, libglib2.0-0, systemd, xdg-utils
- [ ] Description is present and accurate
- [ ] Maintainer information is set

## Linting

Run lintian to check for Debian policy compliance:
```bash
lintian calendar-notifications-modal_*.deb
```

Common warnings you might see (acceptable):
- `no-copyright-file` - We use cargo-deb which handles this differently
- `binary-without-manpage` - Man pages are optional for simple applications
- `systemd-service-file-outside-lib` - We use user services which is correct

Critical errors to fix:
- Missing dependencies
- Incorrect file permissions
- Policy violations

## Installation Testing

### On Clean System (Recommended)

Test on a fresh Ubuntu/Debian VM or container:

```bash
# Install the package
sudo dpkg -i calendar-notifications-modal_*.deb

# Fix any missing dependencies
sudo apt --fix-broken install

# Verify binary is in PATH
which calendar-notifications-modal
calendar-notifications-modal --help

# Enable and start service
systemctl --user enable calendar-notifications-modal.service
systemctl --user start calendar-notifications-modal.service

# Check service status
systemctl --user status calendar-notifications-modal.service

# Verify config was created
ls -la ~/.config/calendar-notifications-modal/config.toml

# Check data directory
ls -la ~/.local/share/calendar-notifications-modal/
```

### On Development System

If testing on your development system:

```bash
# Clean any existing installation first
sudo apt remove calendar-notifications-modal || true
rm -rf ~/.config/calendar-notifications-modal
rm -rf ~/.local/share/calendar-notifications-modal

# Then follow installation steps above
```

## Functional Testing

- [ ] Service starts without errors
- [ ] Config file is created with default values
- [ ] Binary responds to `--help` flag
- [ ] Binary accepts configuration file path
- [ ] Modal window appears when events are due (requires configured calendar)
- [ ] Service restarts on failure
- [ ] Service starts on boot (test after reboot)

```bash
# Test binary directly
calendar-notifications-modal --help

# Add a test ICS backend to config
nano ~/.config/calendar-notifications-modal/config.toml

# Restart service
systemctl --user restart calendar-notifications-modal.service

# Watch logs
journalctl --user -u calendar-notifications-modal.service -f
```

## Dependency Verification

Verify all runtime dependencies are properly declared:

```bash
# Check what the binary actually needs
ldd /usr/bin/calendar-notifications-modal

# Verify dependencies are installed
dpkg -l | grep libgtk-4-1
dpkg -l | grep libglib2.0-0
dpkg -l | grep systemd
dpkg -l | grep xdg-utils
```

## Uninstallation Testing

- [ ] Remove package: `sudo apt remove calendar-notifications-modal`
- [ ] Verify binary is removed: `which calendar-notifications-modal` (should fail)
- [ ] Verify service is stopped: `systemctl --user status calendar-notifications-modal.service`
- [ ] Verify config and data remain: `ls ~/.config/calendar-notifications-modal/`
- [ ] Purge package: `sudo apt purge calendar-notifications-modal`
- [ ] Verify package is fully removed: `dpkg -l | grep calendar-notifications-modal`

## Cross-Distribution Testing

Test on multiple Debian/Ubuntu versions:

- [ ] Ubuntu 24.04 LTS (Noble)
- [ ] Ubuntu 22.04 LTS (Jammy)
- [ ] Debian 12 (Bookworm)
- [ ] Debian 13 (Trixie)

Note: Ubuntu 20.04 and Debian 11 may not have GTK4 in default repositories.

## Size Verification

Check package size is reasonable:

```bash
ls -lh calendar-notifications-modal_*.deb
```

Expected size: ~5-10MB (significantly smaller than snap's ~100-200MB)

## Sharing Test

- [ ] Copy .deb file to another machine
- [ ] Verify it installs without access to source repository
- [ ] Test installation on colleague's system

## Performance Testing

- [ ] Verify service memory usage is reasonable: `systemctl --user status calendar-notifications-modal.service`
- [ ] Check startup time: `systemd-analyze --user blame | grep calendar`
- [ ] Verify no memory leaks over extended runtime

## Documentation Validation

- [ ] README.md mentions Debian package installation
- [ ] DEBIAN_INSTALL.md provides clear instructions
- [ ] debian/README.md explains package structure
- [ ] All documentation is accurate and up-to-date

## Checklist Summary

Before distribution, ensure:
- ✅ Package builds successfully
- ✅ All files are included with correct permissions
- ✅ Dependencies are properly declared
- ✅ Lintian shows no critical errors
- ✅ Installation works on clean system
- ✅ Service starts and runs correctly
- ✅ Config is auto-generated
- ✅ Uninstallation is clean
- ✅ Documentation is complete and accurate
- ✅ Package size is reasonable

## Automation

Consider creating a CI/CD pipeline to automate these checks:

```yaml
# Example GitHub Actions workflow
name: Build and Test Debian Package
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v2
      - name: Install Rust
        uses: actions-rs/toolchain@v1
      - name: Install cargo-deb
        run: cargo install cargo-deb
      - name: Build package
        run: make build-deb
      - name: Lint package
        run: lintian *.deb
      - name: Upload artifact
        uses: actions/upload-artifact@v2
        with:
          name: debian-package
          path: "*.deb"
```

## Sign-off

Package validated by: __________________  
Date: __________________  
Distribution approved: [ ] Yes [ ] No  
