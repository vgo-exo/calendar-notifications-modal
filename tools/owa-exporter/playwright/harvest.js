// harvest.js — shape-based calendar event extraction from OWA calendar API
// responses. Kept dependency-free (plain Node, no browser globals) so it can
// run both inside a Playwright
// page.on('response') handler (as plain data, not injected page code) and be
// unit-tested directly with `node harvest.js` style requires.

'use strict';

/**
 * Recursively walk a JSON value collecting anything that looks like a
 * calendar event. Depth-limited to keep this cheap on large payloads.
 * @param {any} node
 * @param {Array<object>} out
 * @param {number} depth
 */
function harvest(node, out, depth) {
  if (!node || depth > 8) return;
  if (Array.isArray(node)) {
    for (const item of node) harvest(item, out, depth + 1);
    return;
  }
  if (typeof node !== 'object') return;

  const ev = tryNormalize(node);
  if (ev) out.push(ev);

  for (const k in node) {
    const v = node[k];
    if (v && typeof v === 'object') harvest(v, out, depth + 1);
  }
}

function tryNormalize(o) {
  // Graph / Outlook REST shape: { subject, start:{dateTime,timeZone}, end:{...}, ... }
  if (o.start && (o.start.dateTime || o.start.DateTime) && (o.subject !== undefined || o.Subject !== undefined)) {
    const start = parseWhen(o.start);
    const end = parseWhen(o.end);
    if (start == null) return null;
    return finalize({
      uid: o.id || o.iCalUId || o.ICalUId || o.seriesMasterId || null,
      title: o.subject || o.Subject || '',
      start,
      end,
      location: pickLocation(o),
      joinUrl: pickJoinUrl(o),
      reminderMin: pickReminder(o),
      cancelled: !!(o.isCancelled || o.IsCancelled),
    });
  }

  // EWS / classic OWA shape: { Subject, Start, End, ... } where Start is ISO or /Date(ms)/
  const S = o.Start || o.start;
  const hasSubjecty = o.Subject !== undefined || o.subject !== undefined || o.Title !== undefined;
  if (S && hasSubjecty && (typeof S === 'string')) {
    const start = parseWhen(S);
    const end = parseWhen(o.End || o.end);
    if (start == null) return null;
    return finalize({
      uid: (o.ItemId && (o.ItemId.Id || o.ItemId.id)) || o.UID || o.Id || o.id || null,
      title: o.Subject || o.subject || o.Title || '',
      start,
      end,
      location: pickLocation(o),
      joinUrl: pickJoinUrl(o),
      reminderMin: pickReminder(o),
      cancelled: false,
    });
  }

  // GraphQL/Apollo-ish shape variants: startTime/endTime as ISO strings, or
  // title instead of subject.
  const titleish = o.subject ?? o.Subject ?? o.title ?? o.Title ?? o.summary;
  const startish = o.startTime ?? o.StartTime ?? o.start ?? o.Start ?? o.dtstart;
  if (titleish !== undefined && startish != null) {
    const start = parseWhen(startish);
    const endish = o.endTime ?? o.EndTime ?? o.end ?? o.End ?? o.dtend;
    const end = parseWhen(endish);
    if (start == null) return null;
    return finalize({
      uid: o.id || o.Id || o.eventId || o.EventId || o.iCalUId || o.ICalUId || null,
      title: titleish || '',
      start,
      end,
      location: pickLocation(o),
      joinUrl: pickJoinUrl(o),
      reminderMin: pickReminder(o),
      cancelled: false,
    });
  }

  return null;
}

function finalize(ev) {
  // Some non-event objects (e.g. working-hours/availability config) coincidentally
  // have Start/End + an empty Subject/Title key, which would otherwise show up as
  // bogus full-day "(no title)" reminders. Drop rather than fabricate a title.
  if (!ev.title || !String(ev.title).trim()) return null;
  if (!ev.uid) ev.uid = 'gen-' + hash(ev.title + '|' + ev.start);
  if (ev.end == null || ev.end < ev.start) ev.end = ev.start + 30 * 60 * 1000;
  return ev;
}

function pickLocation(o) {
  const loc = o.location || o.Location;
  if (!loc) return '';
  if (typeof loc === 'string') return loc;
  return loc.displayName || loc.DisplayName || loc.Name || '';
}

function pickJoinUrl(o) {
  const om = o.onlineMeeting || o.OnlineMeeting;
  if (om && (om.joinUrl || om.JoinUrl)) return om.joinUrl || om.JoinUrl;
  const cands = [
    o.onlineMeetingUrl, o.OnlineMeetingUrl,
    o.JoinOnlineMeetingUrl, o.joinOnlineMeetingUrl,
    o.OnlineMeetingJoinUrl, o.onlineMeetingJoinUrl,
    o.SkypeTeamsMeetingUrl, o.skypeTeamsMeetingUrl,
  ];
  for (const c of cands) if (c && typeof c === 'string') return c;
  const body = (o.body && (o.body.content || o.body.Content)) || o.Body && (o.Body.Value || o.Body) || o.bodyPreview || o.BodyPreview || '';
  const m = String(body).match(
    /https:\/\/teams\.microsoft\.com\/l\/meetup-join\/[^\s"'<>)]+|https:\/\/teams\.microsoft\.com\/meet\/[^\s"'<>)]+|https:\/\/teams\.live\.com\/meet\/[^\s"'<>)]+|https:\/\/[A-Za-z0-9.-]*zoom\.us\/(?:j|my|w)\/[^\s"'<>)]+|https:\/\/meet\.google\.com\/[a-z-]+/i
  );
  return m ? m[0] : '';
}

function pickReminder(o) {
  const on = o.isReminderOn !== undefined ? o.isReminderOn
           : o.IsReminderSet !== undefined ? o.IsReminderSet
           : undefined;
  const mins = o.reminderMinutesBeforeStart != null ? o.reminderMinutesBeforeStart
             : o.ReminderMinutesBeforeStart != null ? o.ReminderMinutesBeforeStart
             : null;
  if (on === false) return null;
  if (mins != null && !isNaN(+mins) && +mins >= 0) return +mins;
  return null;
}

// Parse a "when" value into epoch ms (UTC), or null.
function parseWhen(w) {
  if (w == null) return null;
  if (typeof w === 'object') {
    const dt = w.dateTime || w.DateTime;
    const tz = w.timeZone || w.TimeZone;
    if (!dt) return null;
    return parseDateTime(dt, tz);
  }
  if (typeof w === 'string') return parseDateTime(w, null);
  return null;
}

function parseDateTime(s, tz) {
  const m = String(s).match(/\/Date\((\d+)/);
  if (m) return parseInt(m[1], 10);

  if (/[zZ]$|[+-]\d{2}:?\d{2}$/.test(s)) {
    const t = Date.parse(s);
    return isNaN(t) ? null : t;
  }

  if (tz && /^utc$/i.test(tz)) {
    const t = Date.parse(s.replace(/(\.\d+)?$/, '') + 'Z');
    return isNaN(t) ? null : t;
  }
  const t = Date.parse(s);
  return isNaN(t) ? null : t;
}

function hash(s) {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(36);
}

module.exports = { harvest, tryNormalize, parseWhen, parseDateTime };
