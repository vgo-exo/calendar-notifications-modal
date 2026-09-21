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
// Sidecar the daemon can read to know *why* the .ics wasn't refreshed, since a
// failed run otherwise leaves the .ics untouched with no error of its own.
const STATUS_PATH = TARGET_ICS + '.status.json';
const HORIZON_DAYS = parseInt(process.env.OWA_PW_HORIZON_DAYS || '14', 10);
const NAV_HOPS = parseInt(process.env.OWA_PW_NAV_HOPS || '2', 10); // best-effort "next week" clicks
const HEADLESS = process.env.OWA_PW_HEADLESS !== '0';
const NAV_TIMEOUT_MS = parseInt(process.env.OWA_PW_NAV_TIMEOUT_MS || '45000', 10);
// When set, every event-shaped JSON response is written verbatim to this dir so
// the raw OWA payload (and where/whether a join URL appears) can be inspected.
const DEBUG_DUMP_DIR = process.env.OWA_PW_DEBUG_DUMP || '';
// The calendar grid omits join URLs; OWA fetches them per event via a
// `GetCalendarEvent` POST. Set to 0 to disable that follow-up enrichment.
const ENRICH_JOIN_URLS = process.env.OWA_PW_ENRICH !== '0';
// Cap enrichment requests per run to keep the export cheap.
const ENRICH_MAX = parseInt(process.env.OWA_PW_ENRICH_MAX || '60', 10);

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

// Atomically record the outcome of this run for the Rust daemon to check.
function writeStatus(ok, message) {
  try {
    fs.mkdirSync(path.dirname(STATUS_PATH), { recursive: true });
    const tmp = STATUS_PATH + '.tmp-' + process.pid;
    fs.writeFileSync(
      tmp,
      JSON.stringify({ ok, timestamp: new Date().toISOString(), message }),
      'utf8'
    );
    fs.renameSync(tmp, STATUS_PATH);
  } catch (e) {
    console.error('failed to write status sidecar:', e);
  }
}

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

// A grid event whose location/title hints at an online meeting is worth a
// follow-up GetCalendarEvent call to fetch its join URL.
function likelyOnline(ev) {
  const hay = ((ev.location || '') + ' ' + (ev.title || '')).toLowerCase();
  return /teams|zoom|meet|r[ée]union|visio|online|webex|skype|hangout/.test(hay);
}

// Pull a join URL out of a GetCalendarEvent response: prefer the structured
// OnlineMeetingJoinUrl field, else scan for a recognized provider link.
function extractJoinUrl(node) {
  let found = '';
  (function walk(n, d) {
    if (found || !n || d > 14) return;
    if (Array.isArray(n)) { for (const x of n) walk(x, d + 1); return; }
    if (typeof n !== 'object') return;
    for (const k in n) {
      const v = n[k];
      if (typeof v === 'string' && v && /onlinemeetingjoinurl/i.test(k) && /^https?:/i.test(v)) {
        found = v;
        return;
      }
    }
    for (const k in n) if (n[k] && typeof n[k] === 'object') walk(n[k], d + 1);
  })(node, 0);
  if (found) return found;
  const m = JSON.stringify(node).match(
    /https:\/\/teams\.microsoft\.com\/(?:l\/meetup-join|meet)\/[^\s"'<>)\\]+|https:\/\/teams\.live\.com\/meet\/[^\s"'<>)\\]+|https:\/\/[A-Za-z0-9.-]*zoom\.us\/(?:j|my|w)\/[^\s"'<>)\\]+|https:\/\/meet\.google\.com\/[a-z-]+/i
  );
  return m ? m[0] : '';
}

// Build a GetCalendarEvent body for one event id by cloning a captured template
// request (which carries the exact ItemShape OWA expects) and swapping EventIds.
function buildGetEventBody(templateBody, itemId) {
  const obj = JSON.parse(templateBody);
  if (obj && obj.Body) {
    obj.Body.EventIds = [{ __type: 'ItemId:#Exchange', Id: itemId }];
  }
  return JSON.stringify(obj);
}

// Copy the template headers, dropping ones the request layer must set itself.
function replayHeaders(headers) {
  const out = {};
  for (const k in headers) {
    if (/^(cookie|content-length|host|connection|accept-encoding)$/i.test(k)) continue;
    out[k] = headers[k];
  }
  return out;
}

// Replay GetCalendarEvent for each online-looking event missing a join URL and
// fill it in. Uses the browser context's request API so cookies/session apply.
async function enrichJoinUrls(context, events, template) {
  let enriched = 0;
  let attempts = 0;
  for (const ev of events.values()) {
    if (attempts >= ENRICH_MAX) break;
    if (ev.joinUrl) continue;
    if (!ev.uid || /^gen-/.test(ev.uid)) continue;
    if (!likelyOnline(ev)) continue;
    attempts++;
    try {
      const resp = await context.request.post(template.url, {
        headers: replayHeaders(template.headers),
        data: buildGetEventBody(template.body, ev.uid),
      });
      if (!resp.ok()) continue;
      const url = extractJoinUrl(await resp.json());
      if (url) {
        ev.joinUrl = url;
        enriched++;
      }
    } catch (_) {
      // one failed enrichment shouldn't abort the rest
    }
  }
  return { enriched, attempts };
}

async function main() {
  if (!fs.existsSync(PROFILE_DIR)) {
    console.error(
      'No browser profile found at ' + PROFILE_DIR + '.\n' +
      'Run `npm run login` first to sign in interactively.'
    );
    writeStatus(false, 'not signed in — run: npm run login');
    process.exit(2);
  }

  const events = new Map(); // uid -> normalized event

  const context = await chromium.launchPersistentContext(PROFILE_DIR, {
    headless: HEADLESS,
    channel: 'chrome',
  });

  let sawAnyJson = 0;
  const page = context.pages()[0] || (await context.newPage());

  // Capture one real GetCalendarEvent request to replay for per-event join URLs.
  let getEventTemplate = null;
  page.on('request', (req) => {
    try {
      if (getEventTemplate) return;
      const h = req.headers();
      const action = h['action'] || h['x-owa-actionsource'] || '';
      if (req.method() !== 'POST' || action !== 'GetCalendarEvent') return;
      const pd = req.postData();
      if (pd && /EventIds/.test(pd)) {
        getEventTemplate = { url: req.url(), headers: h, body: pd };
      }
    } catch (_) {
      // template capture is best-effort
    }
  });

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
      if (DEBUG_DUMP_DIR) {
        try {
          fs.mkdirSync(DEBUG_DUMP_DIR, { recursive: true });
          const name = `resp-${String(sawAnyJson).padStart(3, '0')}.json`;
          fs.writeFileSync(path.join(DEBUG_DUMP_DIR, name), text, 'utf8');
          const req = resp.request();
          const meta = {
            url: req.url(),
            method: req.method(),
            action: (req.headers()['action'] || ''),
            headers: req.headers(),
            postData: (req.postData() || '').slice(0, 4000),
          };
          fs.writeFileSync(path.join(DEBUG_DUMP_DIR, `resp-${String(sawAnyJson).padStart(3, '0')}.req.json`), JSON.stringify(meta, null, 2), 'utf8');
        } catch (_) {
          // debug dump is best-effort
        }
      }
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
      writeStatus(false, 'session expired — run: npm run login');
      await context.close();
      process.exit(3);
    }

    for (let i = 0; i < NAV_HOPS; i++) {
      const clicked = await clickNext(page);
      if (!clicked) break;
      await page.waitForLoadState('networkidle', { timeout: NAV_TIMEOUT_MS }).catch(() => {});
      await page.waitForTimeout(1500);
    }

    if (ENRICH_JOIN_URLS && getEventTemplate) {
      const { enriched, attempts } = await enrichJoinUrls(context, events, getEventTemplate);
      console.log(`OWA export: enriched ${enriched}/${attempts} online event(s) with join URLs.`);
    } else if (ENRICH_JOIN_URLS) {
      console.warn('OWA export: no GetCalendarEvent template captured; join URLs not enriched.');
    }
  } finally {
    await context.close();
  }

  // Prune to horizon + drop stale past events, mirroring the userscript.
  // sawAnyJson === 0 means the page never fired any calendar-shaped network
  // traffic at all — a soft auth failure (session borderline-expired, no hard
  // login redirect) rather than a genuinely empty calendar. Treat it as a
  // failure and leave the previously-captured .ics untouched instead of
  // overwriting real events with an empty calendar.
  if (sawAnyJson === 0) {
    console.error(
      'OWA export: saw 0 JSON response(s) — no calendar data was fetched at all ' +
      '(likely a borderline-expired session). Leaving existing work.ics untouched.'
    );
    writeStatus(false, 'no calendar data captured — session likely stale, run: npm run login');
    process.exit(4);
  }

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
  writeStatus(true, `captured ${events.size} event(s)`);
  if (events.size === 0) {
    console.warn(
      'WARNING: JSON responses were seen but 0 events were harvested from them. ' +
      'Set OWA_PW_HEADLESS=0 and re-run to watch the browser interactively, or ' +
      'capture a sample response with OWA_PW_DEBUG_DUMP to tune harvest.js.'
    );
  }
}

main().catch((err) => {
  console.error('export.js failed:', err);
  writeStatus(false, 'unexpected error: ' + err.message);
  process.exit(1);
});
