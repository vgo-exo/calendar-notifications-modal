#!/usr/bin/env node
// login.js — one-time interactive OWA sign-in.
//
// Opens a real, visible Chrome window bound to a persistent profile directory
// and lets you sign in normally (including MFA / device compliance prompts).
// The profile directory *is* the session store — Chrome persists cookies and
// local storage there automatically, so nothing else needs to be exported.
// Re-run this whenever export.js reports the session has expired.

'use strict';

const { chromium } = require('playwright');
const os = require('os');
const path = require('path');

const PROFILE_DIR =
  process.env.OWA_PW_PROFILE_DIR ||
  path.join(os.homedir(), '.local/share/calendar-notifications-modal/owa-playwright-profile');
const CALENDAR_URL =
  process.env.OWA_PW_CALENDAR_URL || 'https://outlook.office.com/calendar/view/week';

async function main() {
  console.log('Profile dir:', PROFILE_DIR);
  console.log('Launching Chrome — sign in to your Outlook account in the window that opens.');
  console.log('When your calendar has loaded, just close the browser window to finish.\n');

  const context = await chromium.launchPersistentContext(PROFILE_DIR, {
    headless: false,
    channel: 'chrome', // use the real, already-trusted Chrome install
    viewport: null,
    args: ['--start-maximized'],
  });

  const page = context.pages()[0] || (await context.newPage());
  await page.goto(CALENDAR_URL, { waitUntil: 'domcontentloaded' }).catch(() => {});

  await new Promise((resolve) => {
    context.on('close', resolve);
  });

  console.log('\nSession saved. You can now run the exporter (npm run export) unattended.');
}

main().catch((err) => {
  console.error('login.js failed:', err);
  process.exit(1);
});
