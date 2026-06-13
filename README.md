# Calendar Notifications Modal

An Outlook-style **upcoming-event reminder modal** for Linux (GNOME / Wayland).

A small background daemon polls your calendars and, when events approach, pops a
true modal window listing everything that's currently due. Each event can be
**dismissed**, **snoozed** (with a context-aware dropdown), or **joined** when an
online meeting link is detected. A **Dismiss all** button clears them at once.

Built in Rust with GTK4. Reliable by design: a single long-running process owns
both the polling loop and the window (no IPC), and dismiss/snooze state is
persisted in SQLite so it survives restarts.

## Features

- Scrollable list of due events in a modal window.
- An event becomes eligible when `now >= start − reminder`, using the event's own
  reminder time when present, or a configurable global default otherwise.
- Per-event **Dismiss**.
- Per-event **Snooze** with a context-aware dropdown (multiples of 5 minutes):
  - Before start: *"Remind me X minutes before start"* (only future-valid choices).
  - After start: *"Remind me in X minutes"*.
- Per-event **Join** for **Microsoft Teams**, **Google Meet**, and **Zoom**
  links (opened via `xdg-open`).
- **Dismiss all** footer button.
- Pluggable calendar backends. Implemented today: **ICS** (local file or HTTP(S)
  URL), **Microsoft Graph** (Microsoft 365 / Outlook), and **Google Calendar**.
  Planned: CalDAV.

## Architecture

A Cargo workspace of focused crates:

| Crate          | Responsibility |
|----------------|----------------|
| `cnm-core`     | Domain types, meeting-link detection, the `CalendarBackend` trait |
| `cnm-store`    | SQLite persistence of reminder state (dismiss / snooze) |
| `cnm-engine`   | Pure eligibility, snooze-option, and scheduling logic (unit-tested) |
| `cnm-backends` | Backend implementations (ICS, Microsoft Graph, Google) |
| `cnm-ui`       | GTK4 modal window |
| `cnm-app`      | The daemon: config + polling worker + main loop, produces the binary `calendar-notifications-modal` |

The polling worker runs on its own thread (a Tokio runtime) and refreshes a
shared event snapshot. The GTK main thread re-evaluates the due set on a timer
and reconciles the window. All UI state and the SQLite store live on the main
thread.

## Requirements

- Rust (stable) — install via <https://rustup.rs>.
- GTK4 development libraries:

  ```bash
  sudo apt install -y libgtk-4-dev build-essential   # Debian/Ubuntu
  ```

## Build & test

```bash
cargo build --release
cargo test --workspace
```

The binary is produced at `target/release/calendar-notifications-modal`.

## Install

### Easy Install: Debian Package (Best for Ubuntu/Debian) 📦

The easiest way to install on Debian/Ubuntu systems:

```bash
# Build the package
make build-deb

# Install locally
make install-deb

# Enable and start
systemctl --user enable --now calendar-notifications-modal.service
```

**Benefits:**
- ✅ Small file size (~5-10MB vs snap's 100-200MB)
- ✅ Fast installation with native package manager
- ✅ Easy to share with coworkers via .deb file
- ✅ No snapd required

Edit the config and restart:
```bash
nano ~/.config/calendar-notifications-modal/config.toml
systemctl --user restart calendar-notifications-modal.service
```

See [DEBIAN_INSTALL.md](DEBIAN_INSTALL.md) for detailed Debian package guide.

### Alternative: Snap Package 🚀

For non-Debian systems or Snap Store distribution:

```bash
# Build the snap
snapcraft

# Install locally
sudo snap install --dangerous calendar-notifications-modal_*.snap
```

**That's it!** The snap automatically:
- ✅ Generates a default config at `~/.config/calendar-notifications-modal/config.toml`
- ✅ Starts on boot with your graphical session
- ✅ Restarts on failure

Edit the config and restart:
```bash
nano ~/.config/calendar-notifications-modal/config.toml
systemctl --user restart snap.calendar-notifications-modal.daemon.service
```

See [SNAP_INSTALL.md](SNAP_INSTALL.md) for detailed snap installation guide.

### Manual Install: systemd user service

```bash
./packaging/install.sh
```

This builds the release binary, installs it to `~/.local/bin`, installs the
systemd **user** unit, imports the graphical-session environment, and enables
the service. On first run a sample config is written if none exists.

Manage it with:

```bash
systemctl --user status  calendar-notifications-modal.service
systemctl --user restart calendar-notifications-modal.service
journalctl --user -u calendar-notifications-modal.service -f
```

> **Wayland note:** a systemd user service needs the session environment
> (`WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`). The install
> script imports these; if you start the service before logging into your
> graphical session you may need to re-run
> `systemctl --user import-environment ...` (see `install.sh`).

## Configuration

Config lives at `~/.config/calendar-notifications-modal/config.toml`. See
[`packaging/config.example.toml`](packaging/config.example.toml). Example:

```toml
poll_interval_secs = 60
refresh_interval_secs = 15
global_reminder_minutes = 15

[snooze]
before_start = [5, 10, 15, 30, 60]
after_now = [5, 10, 15, 30]

[[backends]]
type = "ics"
id = "work"
url = "https://example.com/calendar.ics"

[[backends]]
type = "msgraph"
id = "outlook"
client_id = "00000000-0000-0000-0000-000000000000"
tenant = "common"

[[backends]]
type = "google"
id = "gcal"
client_id = "xxxxxxxx.apps.googleusercontent.com"
client_secret = "your-google-client-secret"
calendar_id = "primary"
```

### Backends

- **ics** — set either `url` (HTTP/HTTPS) or `file` (local path). Reminders are
  read from `VALARM` `TRIGGER` durations; meeting links are detected from the
  event location, description, and conferencing fields.
- **msgraph** — a real Microsoft 365 / Outlook calendar via the Microsoft Graph
  API. Uses the OAuth2 **device-code** flow with your own Azure app registration
  (no client secret). See setup below. `tenant` defaults to `common` (personal +
  work/school accounts).

#### Microsoft Graph setup

1. In the [Azure portal](https://portal.azure.com) → **Entra ID** → **App
   registrations** → **New registration**. Give it a name; under *Supported
   account types* pick the option matching your `tenant` (the default `common`
   maps to "Accounts in any organizational directory and personal Microsoft
   accounts").
2. Open the app → **Authentication** → enable **Allow public client flows**
   (*"Enable the following mobile and desktop flows"* = **Yes**).
3. **API permissions** → **Add a permission** → **Microsoft Graph** →
   **Delegated permissions** → add **Calendars.Read** (the daemon only reads).
4. Copy the **Application (client) ID** into your `config.toml` as `client_id`.
5. Sign in once from a terminal:

   ```bash
   calendar-notifications-modal --login outlook   # the backend `id`
   ```

   This prints a URL and a short code; open the URL, enter the code, and approve.
   Tokens are cached at
   `~/.local/share/calendar-notifications-modal/msgraph-<id>.json` (`0600`) and
   refreshed automatically. Re-run `--login` if you ever revoke access.

Planned backends (CalDAV) will require per-provider app registration; they are
not yet implemented and unknown backend types are skipped with a warning if
present in the config.

#### Google Calendar setup

A personal Google account works with no admin involvement (you own the project):

1. Go to the [Google Cloud Console](https://console.cloud.google.com) → create a
   project (or reuse one).
2. **APIs & Services** → **Library** → enable **Google Calendar API**.
3. **APIs & Services** → **OAuth consent screen**: choose **External**, fill the
   required fields, and add your own Google address under **Test users**.
4. **APIs & Services** → **Credentials** → **Create credentials** → **OAuth
   client ID** → application type **TVs and Limited Input devices**. Copy the
   **Client ID** and **Client secret** into `config.toml` (`client_id`,
   `client_secret`). For installed/limited-input apps this "secret" is not truly
   confidential, but still keep your config file private.
5. Sign in once:

   ```bash
   calendar-notifications-modal --login gcal   # the backend `id`
   ```

   Open the printed URL, enter the code, and approve. Tokens are cached at
   `~/.local/share/calendar-notifications-modal/google-<id>.json` (`0600`).
   `calendar_id` defaults to `primary`.

   > While the consent screen is in **Testing** mode, Google refresh tokens
   > expire after 7 days, so you'll need to re-run `--login` weekly. Publishing
   > the consent screen removes that limit.

## State

Reminder state (dismissed / snoozed-until) is stored in
`~/.local/share/calendar-notifications-modal/state.db`. Deleting this file
resets all dismissals and snoozes.

Microsoft Graph OAuth tokens are cached alongside it as
`~/.local/share/calendar-notifications-modal/msgraph-<id>.json` (`0600`). Delete
a token file to force re-authentication of that backend (re-run `--login`).

## Development

Run the daemon directly with verbose logging and an isolated config:

```bash
RUST_LOG=debug \
XDG_CONFIG_HOME=/tmp/cnm/cfg XDG_DATA_HOME=/tmp/cnm/data \
  cargo run --bin calendar-notifications-modal
```

## License

MIT.
