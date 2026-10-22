// Inputs for the ICS differential test. Each case is run by the JavaScript
// parser (generating the expected output) and by the Rust parser (which must
// match). Add a case here, run `node crates/blink-core/tests/diff/gen-ics.mjs`,
// commit the regenerated fixtures.

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const moodle = fs.readFileSync(path.join(here, '..', '..', '..', '..', 'test', 'fixtures', 'moodle-sample.ics'), 'utf8')

const D = 24 * 3600 * 1000
const utc = (y, m, d, h = 0, mi = 0, s = 0) => Date.UTC(y, m - 1, d, h, mi, s)

const wrap = (...events) => `BEGIN:VCALENDAR\r\nVERSION:2.0\r\n${events.join('')}END:VCALENDAR\r\n`
const ev = (lines) => `BEGIN:VEVENT\r\n${lines.join('\r\n')}\r\nEND:VEVENT\r\n`
// Shorthand: an event with the given DTSTART line and optional extra lines.
const at = (dtstart, uid = 'u1', summary = 'Task is due', extra = []) =>
  ev([`UID:${uid}`, `SUMMARY:${summary}`, dtstart, ...extra])

const NOW = utc(2026, 8, 17)

export const CASES = []
const add = (name, ics, opts = {}) => CASES.push({ name, ics, nowMs: NOW, ...opts })

// ---- the real Moodle sample
add('moodle sample, early', moodle)
add('moodle sample, after the first deadlines', moodle, { nowMs: utc(2026, 8, 23) })
add('moodle sample, limit 2', moodle, { limit: 2 })

// ---- inputs that are not calendars
add('empty', '')
add('whitespace', '   \r\n  ')
add('no VCALENDAR', 'BEGIN:VEVENT\r\nUID:x\r\nSUMMARY:A\r\nDTSTART:20260822T170000Z\r\nEND:VEVENT\r\n')
add('calendar with no events', 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n')

// ---- line endings, folding, escapes
add('LF line endings', wrap(at('DTSTART:20260822T170000Z')).replace(/\r\n/g, '\n'))
add('folded with space and tab', wrap(ev(['UID:fold', 'SUMMARY:Very long', ' title that\r\n\twraps is due', 'DTSTART:20260822T170000Z'])).replace('Very long\r\n title', 'Very long\r\n title'))
add('escapes in summary', wrap(at('DTSTART:20260822T170000Z', 'esc', 'A\\, B\\; C\\\\ D\\nE is due')))
add('trailing backslash', wrap(at('DTSTART:20260822T170000Z', 'bs', 'Ends with slash\\')))
add('lowercase markers and names', 'begin:vcalendar\r\nBEGIN:vevent\r\nuid:low\r\nsummary:lower is due\r\ndtstart:20260822T170000Z\r\nend:VEVENT\r\nEND:VCALENDAR\r\n')
add('non-event blocks are ignored', 'BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:t\r\nSUMMARY:todo\r\nDTSTART:20260822T170000Z\r\nEND:VTODO\r\nBEGIN:VEVENT\r\nUID:e\r\nSUMMARY:real is due\r\nDTSTART:20260822T170000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n')
add('quoted parameter containing a colon', wrap(ev(['UID:q', 'SUMMARY:Quoted is due', 'DTSTART;TZID="Asia/Jakarta":20260822T170000', 'ATTENDEE;CN="a:b":mailto:x@y.z'])))
add('parameter values and case', wrap(ev(['UID:p', 'SUMMARY;LANGUAGE=en:Param is due', 'dtstart;tzid=Asia/Jakarta:20260822T170000'])))

// ---- summary suffixes
for (const [suffix] of [
  ['is due for grading'], ['should be submitted'], ['harus diserahkan'], ['is due'], ['tenggat'],
  ['closes'], ['ditutup'], ['opens'], ['dibuka'], ['IS DUE'], ['Is Due For Grading']
]) {
  add(`suffix: ${suffix}`, wrap(at('DTSTART:20260822T170000Z', 'sfx', `Name ${suffix}`)))
}
add('summary that is only a suffix', wrap(at('DTSTART:20260822T170000Z', 's1', 'is due')))
add('summary with HTML and entities', wrap(at('DTSTART:20260822T170000Z', 's2', 'Tugas &amp; <b>Kuis</b> &#8211; bab&nbsp;1 is due')))
add('unicode title', wrap(at('DTSTART:20260822T170000Z', 'uni', 'Ujian Akhir Semester – Pemrograman éü中文 is due')))

// ---- descriptions
add('description with HTML', wrap(at('DTSTART:20260822T170000Z', 'd1', 'D is due', ['DESCRIPTION:<p>Kuis online bab 4</p><br/>Baris&nbsp;dua &amp; &quot;kutipan&quot; &unknown; &#169; &#0;'])))
const long = ('Paragraf panjang dengan kata-kata. ').repeat(20)
add('description over 240 characters', wrap(at('DTSTART:20260822T170000Z', 'd2', 'D is due', [`DESCRIPTION:${long}`])))
add('description exactly 240', wrap(at('DTSTART:20260822T170000Z', 'd3', 'D is due', [`DESCRIPTION:${'x'.repeat(240)}`])))
add('description 241 with spaces at the cut', wrap(at('DTSTART:20260822T170000Z', 'd4', 'D is due', [`DESCRIPTION:${'x'.repeat(238)}   yy`])))
add('description with an emoji well before the cut', wrap(at('DTSTART:20260822T170000Z', 'd5', 'D is due', [`DESCRIPTION:😀 ${'y'.repeat(300)}`])))

// ---- course
add('course from CATEGORIES with pipe and year', wrap(at('DTSTART:20260822T170000Z', 'c1', 'C is due', ['CATEGORIES:CS101 Algorithms|extra part_2026'])))
add('course with trailing _2026', wrap(at('DTSTART:20260822T170000Z', 'c2', 'C is due', ['CATEGORIES:Basis Data_2026'])))
add('course: empty first category then a real one', wrap(at('DTSTART:20260822T170000Z', 'c3', 'C is due', ['CATEGORIES:_2026', 'CATEGORIES:Jaringan Komputer'])))
add('course from description (English)', wrap(at('DTSTART:20260822T170000Z', 'c4', 'C is due', ['DESCRIPTION:Submit now. Course: Data Structures. More text'])))
add('course from description (Indonesian)', wrap(at('DTSTART:20260822T170000Z', 'c5', 'C is due', ['DESCRIPTION:Mata kuliah - Basis Data; lainnya'])))
add('course from description, kelas', wrap(at('DTSTART:20260822T170000Z', 'c6', 'C is due', ['DESCRIPTION:KELAS: Fisika Dasar 2'])))
add('no course anywhere', wrap(at('DTSTART:20260822T170000Z', 'c7', 'C is due')))

// ---- URL
add('url https', wrap(at('DTSTART:20260822T170000Z', 'l1', 'L is due', ['URL:https://brone.ub.ac.id/mod/assign/view.php?id=1'])))
add('url http is ignored', wrap(at('DTSTART:20260822T170000Z', 'l2', 'L is due', ['URL:http://example.edu/a'])))
add('url uppercase scheme', wrap(at('DTSTART:20260822T170000Z', 'l3', 'L is due', ['URL:HTTPS://example.edu/a'])))
add('url with a space is ignored', wrap(at('DTSTART:20260822T170000Z', 'l4', 'L is due', ['URL:https://example.edu/a b'])))
add('link only in description is ignored', wrap(at('DTSTART:20260822T170000Z', 'l5', 'L is due', ['DESCRIPTION:see https://drive.google.com/x'])))
add('url javascript is ignored', wrap(at('DTSTART:20260822T170000Z', 'l6', 'L is due', ['URL:javascript:alert(1)'])))

// ---- start formats
add('start: UTC', wrap(at('DTSTART:20260822T170000Z')))
add('start: floating', wrap(at('DTSTART:20260822T170000')))
add('start: date only (VALUE=DATE)', wrap(at('DTSTART;VALUE=DATE:20260825')))
add('start: date only (8 digits)', wrap(at('DTSTART:20260825')))
add('start: VALUE=DATE-TIME', wrap(at('DTSTART;VALUE=DATE-TIME:20260822T170000Z')))
add('start: no seconds is treated as midnight, not all-day', wrap(at('DTSTART:20260822T1700')))
add('start: lowercase t', wrap(at('DTSTART:20260822t170000Z')))
add('start: dashes are invalid', wrap(at('DTSTART:2026-08-25')))
add('start: garbage', wrap(at('DTSTART:soon')))
add('start: hour 24 rolls to next day', wrap(at('DTSTART:20260822T240000Z')))
add('start: month 13 rolls to next year', wrap(at('DTSTART:20261301T000000Z'), at('DTSTART:20260822T000000Z', 'u2')), { nowMs: utc(2026, 12, 20), horizonMs: 60 * D })
add('start: Feb 30 rolls into March', wrap(at('DTSTART:20260230T120000Z')), { nowMs: utc(2026, 2, 20) })
add('missing DTSTART is skipped', wrap(ev(['UID:n', 'SUMMARY:No start is due'])))
add('missing SUMMARY is skipped', wrap(ev(['UID:n', 'DTSTART:20260822T170000Z'])))
add('missing UID falls back to title|phase', wrap(ev(['SUMMARY:Nameless is due', 'DTSTART:20260822T170000Z'])))
add('blank UID falls back to title|phase', wrap(ev(['UID:   ', 'SUMMARY:Blank is due', 'DTSTART:20260822T170000Z'])))

// ---- time zones (explicit TZID)
const tz = (id, local) => wrap(at(`DTSTART;TZID=${id}:${local}`, 'tz', 'Zone is due'))
add('tzid Asia/Jakarta', tz('Asia/Jakarta', '20260822T170000'))
add('tzid America/New_York ordinary', tz('America/New_York', '20260822T170000'))
add('tzid America/New_York spring gap (02:30 does not exist)', tz('America/New_York', '20260308T023000'), { nowMs: utc(2026, 3, 1) })
add('tzid America/New_York fall overlap (01:30 happens twice)', tz('America/New_York', '20261101T013000'), { nowMs: utc(2026, 10, 25) })
add('tzid America/New_York just after the fall change', tz('America/New_York', '20261101T020000'), { nowMs: utc(2026, 10, 25) })
add('tzid Europe/London spring gap', tz('Europe/London', '20260329T013000'), { nowMs: utc(2026, 3, 20) })
add('tzid Europe/London autumn overlap', tz('Europe/London', '20261025T013000'), { nowMs: utc(2026, 10, 20) })
add('tzid Australia/Lord_Howe (30 minute shift) gap', tz('Australia/Lord_Howe', '20261004T021500'), { nowMs: utc(2026, 9, 28) })
add('tzid Australia/Lord_Howe overlap', tz('Australia/Lord_Howe', '20260405T014500'), { nowMs: utc(2026, 3, 30) })
add('tzid Asia/Kolkata (+5:30)', tz('Asia/Kolkata', '20260822T170000'))
add('tzid Pacific/Chatham (+12:45/+13:45)', tz('Pacific/Chatham', '20260822T170000'))
add('tzid Asia/Calcutta (old alias)', tz('Asia/Calcutta', '20260822T170000'))
add('tzid US/Eastern (alias)', tz('US/Eastern', '20260822T170000'))
add('tzid lowercase', tz('asia/jakarta', '20260822T170000'))
add('tzid unknown is skipped', tz('Mars/Phobos', '20260822T170000'))
add('tzid Windows name is skipped', tz('Eastern Standard Time', '20260822T170000'))
add('tzid with UTC suffix Z wins', wrap(at('DTSTART;TZID=Asia/Jakarta:20260822T170000Z')))

// ---- floating and all-day (use the machine zone: run under several)
for (const [label, local] of [
  ['US spring gap', '20260308T023000'], ['US fall overlap', '20261101T013000'],
  ['EU spring gap', '20260329T013000'], ['EU autumn overlap', '20261025T013000'],
  ['Lord Howe gap', '20261004T021500'], ['Lord Howe overlap', '20260405T014500']
]) {
  const month = Number(local.slice(4, 6))
  const day = Number(local.slice(6, 8))
  add(`floating at a daylight-saving edge: ${label}`, wrap(at(`DTSTART:${local}`)), { nowMs: utc(2026, month, 1) - 10 * D + 0 * day })
  add(`all-day on a daylight-saving day: ${label}`, wrap(at(`DTSTART;VALUE=DATE:${local.slice(0, 8)}`)), { nowMs: utc(2026, month, 1) - 10 * D })
}

// ---- windows, ordering, duplicates, limits
add('duplicate uid and time is kept once', wrap(at('DTSTART:20260822T170000Z', 'dup', 'First is due'), at('DTSTART:20260822T170000Z', 'dup', 'Second is due')))
add('same uid different time is kept twice', wrap(at('DTSTART:20260822T170000Z', 'same', 'A is due'), at('DTSTART:20260823T170000Z', 'same', 'A is due')))
add('same time orders by title', wrap(at('DTSTART:20260822T170000Z', 'x1', 'Beta is due'), at('DTSTART:20260822T170000Z', 'x2', 'Alpha is due'), at('DTSTART:20260822T170000Z', 'x3', 'Gamma is due')))
add('ordering by time', wrap(at('DTSTART:20260901T000000Z', 'o1'), at('DTSTART:20260820T000000Z', 'o2'), at('DTSTART:20260825T000000Z', 'o3')))
add('overdue within the default window', wrap(at(`DTSTART:${new Date(NOW - 2 * D).toISOString().replace(/[-:]|\.\d{3}/g, '')}`)))
add('overdue past the default window', wrap(at(`DTSTART:${new Date(NOW - 4 * D).toISOString().replace(/[-:]|\.\d{3}/g, '')}`)))
add('overdue with a longer window', wrap(at(`DTSTART:${new Date(NOW - 4 * D).toISOString().replace(/[-:]|\.\d{3}/g, '')}`)), { keepOverdueMs: 10 * D })
add('beyond the horizon', wrap(at(`DTSTART:${new Date(NOW + 121 * D).toISOString().replace(/[-:]|\.\d{3}/g, '')}`)))
add('inside a longer horizon', wrap(at(`DTSTART:${new Date(NOW + 121 * D).toISOString().replace(/[-:]|\.\d{3}/g, '')}`)), { horizonMs: 200 * D })
add('limit 1', wrap(at('DTSTART:20260822T170000Z', 'a1'), at('DTSTART:20260823T170000Z', 'a2')), { limit: 1 })
