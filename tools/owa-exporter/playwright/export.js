#!/usr/bin/env node
// export.js — headless, unattended OWA calendar export (Phase 2).
//
// Reuses the persistent Chrome profile created by `login.js` (same
// user-data-dir) to load Outlook on the web and capture the real calendar
// network responses via Chrome's network layer (CDP), which sees traffic
// regardless of whether OWA's own Service Worker handled the fetch — the
// blocker that defeated the plain-JS userscript approach. Writes the merged
// result straight to the .ics path the daemon reads: no browser download,
// no watcher needed for this path.
//
// Run this on a timer (see calendar-notifications-owa-export.timer).

'use strict';

const { chromium } = require('playwright');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { harvest } = require('./harvest');
const { buildIcs } = require('./ics');

const PROFILE_DIR =
  process.env.OWA_PW_PROFILE_DIR ||
  path.join(os.homedir(), '.local/share/calendar-notifications-modal/owa-playwright-profile');
const CALENDAR_URL =
  process.env.OWA_PW_CALENDAR_URL || 'https://outlook.office.com/calendar/view/week';
const TARGET_ICS =
  process.env.OWA_ICS_TARGET ||
  path.join(os.homedir(), '.local/share/calendar-notifications-modal/work.ics');
const HORIZON_DAYS = parseInt(process.env.OWA_PW_HORIZON_DAYS || '14', 10);
const NAV_HOPS = parseInt(process.env.OWA_PW_NAV_HOPS || '2', 10); // best-effort "next week" clicks
const HEADLESS = process.env.OWA_PW_HEADLESS !== '0';
const NAV_TIMEOUT_MS = parseInt(process.env.OWA_PW_NAV_TIMEOUT_MS || '45000', 10);

// Selectors are best-effort guesses at OWA's "next" calendar navigation
// control; OWA's DOM/aria-labels vary by build/locale, so failures here are
// non-fatal. If this never matches for your tenant, only the currently
// displayed week is captured (still enough for imminent-meeting reminders,
// since export.js re-runs on a short timer and the view naturally rolls
// forward as "today" advances).
const NEXT_SELECTORS = [
  'button[aria-label="Next"]',
  'button[aria-label*="Next" i]',
  'button[title*="Next" i]',
  '[data-icon-name="ChevronRight"]',
];

function looksJsonish(contentType, url) {
  if (contentType && /json/i.test(contentType)) return true;
  return /service\.svc|calendarview|graphql/i.test(url || '');
}

async function clickNext(page) {
  for (const sel of NEXT_SELECTORS) {
    try {
      const el = page.locator(sel).first();
      if (await el.count()) {
        await el.click({ timeout: 3000 });
        return true;
      }
    } catch (_) {
      // try next selector
    }
  }
  return false;
}

async function main() {
  if (!fs.existsSync(PROFILE_DIR)) {
    console.error(
      'No browser profile found at ' + PROFILE_DIR + '.\n' +
      'Run `npm run login` first to sign in interactively.'
    );
    process.exit(2);
  }

  const events = new Map(); // uid -> normalized event

  const context = await chromium.launchPersistentContext(PROFILE_DIR, {
    headless: HEADLESS,
    channel: 'chrome',
  });

  let sawAnyJson = 0;
  const page = context.pages()[0] || (await context.newPage());

  page.on('response', async (resp) => {
    try {
      const url = resp.url();
      const ct = resp.headers()['content-type'] || '';
      if (!looksJsonish(ct, url)) return;
      const text = await resp.text().catch(() => null);
      if (!text || text.length > 12_000_000) return;
      if (!/subject|Subject|summary|dateTime|DateTime|startTime|StartTime|"Start"/i.test(text)) return;
      let json;
      try {
        json = JSON.parse(text);
      } catch (_) {
        return;
      }
      sawAnyJson++;
      const found = [];
      harvest(json, found, 0);
      for (const ev of found) {
        const existing = events.get(ev.uid);
        if (!existing || existing.start !== ev.start || existing.title !== ev.title) {
          events.set(ev.uid, ev);
        }
      }
    } catch (_) {
      // never let a bad response kill the run
    }
  });

  let finalUrl = '';
  try {
    await page.goto(CALENDAR_URL, { waitUntil: 'domcontentloaded', timeout: NAV_TIMEOUT_MS });
    await page.waitForLoadState('networkidle', { timeout: NAV_TIMEOUT_MS }).catch(() => {});
    finalUrl = page.url();

    if (/login\.microsoftonline\.com|\/login/i.test(finalUrl)) {
      console.error(
        'Redirected to login (' + finalUrl + ') — the saved session has expired.\n' +
        'Run `npm run login` again to re-authenticate.'
      );
      await context.close();
      process.exit(3);
    }

    for (let i = 0; i < NAV_HOPS; i++) {
      const clicked = await clickNext(page);
      if (!clicked) break;
      await page.waitForLoadState('networkidle', { timeout: NAV_TIMEOUT_MS }).catch(() => {});
      await page.waitForTimeout(1500);
    }
  } finally {
    await context.close();
  }

  // Prune to horizon + drop stale past events, mirroring the userscript.
  const now = Date.now();
  const maxFuture = now + HORIZON_DAYS * 86400000;
  const keepPastMs = 60 * 60 * 1000;
  const kept = [...events.values()].filter(
    (ev) => ev.end >= now - keepPastMs && ev.start <= maxFuture
  );

  const ics = buildIcs(kept, { calendarName: 'Work (OWA export)' });

  fs.mkdirSync(path.dirname(TARGET_ICS), { recursive: true });
  const tmp = TARGET_ICS + '.tmp-' + process.pid;
  fs.writeFileSync(tmp, ics, 'utf8');
  fs.renameSync(tmp, TARGET_ICS); // atomic on the same filesystem

  console.log(
    `OWA export: saw ${sawAnyJson} JSON response(s), captured ${events.size} event(s), ` +
    `kept ${kept.length} within ${HORIZON_DAYS}d horizon -> ${TARGET_ICS}`
  );
  if (events.size === 0) {
    console.warn(
      'WARNING: 0 events captured. Set OWA_PW_HEADLESS=0 and re-run to watch the ' +
      'browser interactively, or check that the profile is signed in (npm run login).'
    );
  }
}

main().catch((err) => {
  console.error('export.js failed:', err);
  process.exit(1);
});
