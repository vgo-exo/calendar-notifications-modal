# OWA → ICS exporter (browser-session workaround)

A fallback for calendars locked down by corporate IT: when Microsoft Graph API
access, app registration/assignment, and calendar publishing are **all blocked**,
the one channel that still works is **Outlook on the web (OWA) in your browser**.

This tool piggybacks that authenticated session to keep a local `.ics` fresh,
which the `calendar-notifications-modal` daemon reads through its existing `ics`
file backend — so you get the full reminder modal (snooze, dismiss, and **Join**
for detected Teams/Zoom/Meet links) for your work calendar, with **no admin
involvement**.

> This reads only your own calendar, via your own already-authorized OWA session
> — the same data you can see in the browser. Make sure that's compatible with
> your organization's acceptable-use policy.

## How it works

Location: `tools/owa-exporter/playwright/`. It intercepts calendar responses at
the **browser network layer** (Chrome DevTools Protocol, via Playwright), which
sees traffic regardless of whether a Service Worker handled the fetch — the
same layer the Network tab in DevTools uses.

```
 Outlook on the web (real Chrome, headless, your saved login session)
        │  page.on('response') captures every network response
        ▼
 harvest.js  ── shape-based extraction of calendar events (any JSON shape)
        ▼
 ics.js  ── builds RFC5545 .ics
        │  writes directly to ~/.local/share/.../work.ics (atomic)
        ▼
 calendar-notifications-modal daemon  ── ics file backend, re-read every poll
        ▼
 Reminder modal 🎉
```

1. `login.js` opens a real, visible Chrome window bound to a persistent profile
   directory (`~/.local/share/calendar-notifications-modal/owa-playwright-profile`
   by default). You sign in normally — MFA, device checks, whatever your tenant
   requires — then just close the window. The profile directory *is* the saved
   session (cookies + local storage), no export step needed.
2. `export.js` reuses that same profile **headlessly**, loads the OWA calendar
   week view, listens to every network response via `page.on('response')`,
   JSON-parses anything that looks event-shaped, and writes the merged result
   directly to `~/.local/share/calendar-notifications-modal/work.ics`
   (atomically).
3. A systemd **user timer** runs `export.js` every 15 minutes.
4. If the saved session expires (redirected to a login page), `export.js` exits
   with an error telling you to re-run `npm run login`.

No API keys, no tokens, no OAuth app, and nothing for Conditional Access to
block: it *is* your normal, already-trusted browser session (`channel: 'chrome'`,
not bundled Chromium).

## Install

```bash
cd tools/owa-exporter/playwright
./install-phase2.sh   # npm install, installs + enables the systemd timer
npm run login          # one-time interactive sign-in (visible Chrome window)
```

Then add the backend to `~/.config/calendar-notifications-modal/config.toml`:

```toml
[[backends]]
type = "ics"
id = "work"
file = "/home/<you>/.local/share/calendar-notifications-modal/work.ics"
```

Optionally force an immediate run:

```bash
systemctl --user start calendar-notifications-owa-export.service
journalctl --user -u calendar-notifications-owa-export.service -f
```

## Tuning

Environment variables (set them in the systemd unit or ad hoc when running
`node export.js` by hand):

| Variable | Default | Meaning |
|---|---|---|
| `OWA_PW_PROFILE_DIR` | `~/.local/share/.../owa-playwright-profile` | Persistent browser profile (session) |
| `OWA_ICS_TARGET` | `~/.local/share/.../work.ics` | Where the `.ics` is written |
| `OWA_PW_HORIZON_DAYS` | `14` | Drop events further out than this |
| `OWA_PW_NAV_HOPS` | `2` | Best-effort "click next week" attempts to extend the captured range (non-fatal if the selector doesn't match this tenant's OWA build) |
| `OWA_PW_HEADLESS` | `1` | Set to `0` to watch it run (debugging) |

## If 0 events are captured

Run it headed to watch what happens:

```bash
OWA_PW_HEADLESS=0 node export.js
```

Check the printed "saw N JSON response(s), captured M event(s)" line. If JSON
responses are seen but 0 events are captured, the harvesting shape heuristics
need tuning for this tenant's response format — capture a sample response body
(DevTools Network tab → the `service.svc`/`GetCalendarView` request → Response)
and adjust `harvest.js`.

## Limitations

- Chrome/Playwright must run interactively once (`npm run login`) whenever the
  saved session expires (typically tied to your organization's session/MFA
  lifetime).
- Times are exported in UTC; all-day/multi-day handling is best-effort.

## Files

| File | Role |
|------|------|
| `playwright/harvest.js` | Shape-based event extraction |
| `playwright/ics.js` | RFC5545 `.ics` builder |
| `playwright/login.js` | One-time interactive sign-in |
| `playwright/export.js` | Headless export run, writes `.ics` directly |
| `playwright/install-phase2.sh` | npm install + systemd timer setup |
| `playwright/calendar-notifications-owa-export.service` / `.timer` | systemd **user** oneshot + timer |
