#!/usr/bin/env bash
# install-phase2.sh — set up the Playwright-based OWA exporter (Phase 2).
#
# What it does:
#   * npm install the Playwright dependency here;
#   * resolve the `node` binary (works with nvm-managed installs) and bake it
#     into the systemd unit, since systemd --user services don't source
#     nvm/bashrc;
#   * install + enable a systemd user timer that runs export.js every 15 min;
#   * tell you to run `npm run login` once, interactively, before the timer's
#     first automated run will find a usable session.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

echo "==> npm install"
npm install

NODE_BIN="$(command -v node || true)"
if [ -z "$NODE_BIN" ]; then
  # Try common nvm layout if `node` isn't on PATH in this shell.
  NODE_BIN="$(ls "$HOME"/.nvm/versions/node/*/bin/node 2>/dev/null | sort -V | tail -1 || true)"
fi
if [ -z "$NODE_BIN" ]; then
  echo "Could not find a 'node' binary. Install Node.js (e.g. via nvm) and re-run this script." >&2
  exit 1
fi
echo "==> Using node: $NODE_BIN"

UNIT_DIR="$HOME/.config/systemd/user"
mkdir -p "$UNIT_DIR"

sed "s|NODE_BIN_PLACEHOLDER|$NODE_BIN|" \
  "$HERE/calendar-notifications-owa-export.service" > "$UNIT_DIR/calendar-notifications-owa-export.service"
cp -f "$HERE/calendar-notifications-owa-export.timer" "$UNIT_DIR/calendar-notifications-owa-export.timer"

systemctl --user daemon-reload
systemctl --user enable --now calendar-notifications-owa-export.timer

cat <<EOF

============================================================
 Phase 2 installed.
============================================================

One manual step remains — sign in once, interactively:

   cd $HERE
   npm run login

This opens a real Chrome window; sign in to your work account (MFA etc. as
normal), then just close the window once your calendar has loaded. The
session is saved to:
   \$OWA_PW_PROFILE_DIR (default: ~/.local/share/calendar-notifications-modal/owa-playwright-profile)

After that, the timer will export your calendar automatically every 15
minutes to:
   ${OWA_ICS_TARGET:-$HOME/.local/share/calendar-notifications-modal/work.ics}

Check it with:
   systemctl --user list-timers calendar-notifications-owa-export.timer
   journalctl --user -u calendar-notifications-owa-export.service -f

Force a run right now (after logging in) with:
   systemctl --user start calendar-notifications-owa-export.service
============================================================
EOF
