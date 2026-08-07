// ics.js — RFC5545 VCALENDAR builder from normalized events (see harvest.js).

'use strict';

function icsDate(ms) {
  const d = new Date(ms);
  const p = (n) => String(n).padStart(2, '0');
  return (
    d.getUTCFullYear() + p(d.getUTCMonth() + 1) + p(d.getUTCDate()) +
    'T' + p(d.getUTCHours()) + p(d.getUTCMinutes()) + p(d.getUTCSeconds()) + 'Z'
  );
}

function icsText(s) {
  return String(s)
    .replace(/\\/g, '\\\\')
    .replace(/;/g, '\\;')
    .replace(/,/g, '\\,')
    .replace(/\r?\n/g, '\\n');
}

function foldLine(line) {
  if (line.length <= 73) return line;
  let out = '';
  let rest = line;
  while (rest.length > 73) {
    out += rest.slice(0, 73) + '\r\n ';
    rest = rest.slice(73);
  }
  return out + rest;
}

/**
 * @param {Array<object>} events normalized events from harvest.js (uid, title, start, end, location, joinUrl, reminderMin, cancelled)
 * @param {object} [opts]
 * @param {string} [opts.calendarName]
 */
function buildIcs(events, opts) {
  opts = opts || {};
  const now = Date.now();
  const lines = [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'PRODID:-//calendar-notifications-modal//owa-export-playwright//EN',
    'CALSCALE:GREGORIAN',
    'METHOD:PUBLISH',
    'X-WR-CALNAME:' + (opts.calendarName || 'Work (OWA export)'),
  ];
  const sorted = [...events].filter((e) => !e.cancelled).sort((a, b) => a.start - b.start);
  for (const ev of sorted) {
    lines.push('BEGIN:VEVENT');
    lines.push('UID:' + icsText(ev.uid) + '@owa-export');
    lines.push('DTSTAMP:' + icsDate(now));
    lines.push('DTSTART:' + icsDate(ev.start));
    lines.push('DTEND:' + icsDate(ev.end));
    lines.push(foldLine('SUMMARY:' + icsText(ev.title)));
    if (ev.location) lines.push(foldLine('LOCATION:' + icsText(ev.location)));
    if (ev.joinUrl) {
      lines.push(foldLine('DESCRIPTION:' + icsText('Join: ' + ev.joinUrl)));
      lines.push(foldLine('URL:' + icsText(ev.joinUrl)));
    }
    if (ev.reminderMin != null) {
      lines.push('BEGIN:VALARM');
      lines.push('ACTION:DISPLAY');
      lines.push('DESCRIPTION:Reminder');
      lines.push('TRIGGER:-PT' + ev.reminderMin + 'M');
      lines.push('END:VALARM');
    }
    lines.push('END:VEVENT');
  }
  lines.push('END:VCALENDAR');
  return lines.join('\r\n') + '\r\n';
}

module.exports = { buildIcs, icsDate, icsText, foldLine };
