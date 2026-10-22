import { STRINGS } from '../../../../src/lib/i18n.js'
// Inputs for the "ops" differential test: plain function calls on the small
// pure modules. Each case is {fn, args}. The JS side runs the real module; the
// Rust side must return the same JSON. Regenerate with gen-ops.mjs.

export const OPS = []
const add = (fn, ...args) => OPS.push({ fn, args })

// ---- notifyConfig.normalizeThresholdsHours
for (const v of [
  [24, 6, 1], [1, 6, 24], [6, 6, 6], ['6', '24', 1], [0, -1, 5], [337, 336, 335.5], [0.5, 0.25],
  [], ['x', null, true, false], [null], 'nope', 5, null, { a: 1 }, [' 12 ', '0x10', '1e1'], [Number.MAX_SAFE_INTEGER],
  [[], [5], [[6]]], [1.5, 1.5, 2]
]) add('normalizeThresholdsHours', v)

// ---- feeds
for (const v of ['moodle', 'google-1', 'kelas_a.2', 'a', '', 'ada spasi', 'a'.repeat(64), 'a'.repeat(65), '-bad', '.bad', ' padded ', 'ünïcode', 'a/b', 'ok.id_1-2']) add('isValidFeedId', v)
for (const v of ['moodle', 'google', 'ics', ' ics ', 'dropbox', '', 'MOODLE']) add('normalizeFeedKind', v)
const U = 'https://example.com/cal.ics?token=secret'
for (const f of [
  { id: 'x1', kind: 'moodle', url: U },
  { id: 'x1', kind: 'google', url: U, label: '  Kelas  ', enabled: false },
  { id: '', kind: 'moodle', url: U },
  { id: 'a', kind: 'dropbox', url: U },
  { id: 'a', kind: 'ics', url: 'ftp://x/y' },
  { id: 'a', kind: 'ics', url: '' },
  { id: 'a', kind: 'ics', url: '   ' },
  { id: 'a', kind: 'ics', url: 'not a url' },
  { id: 'a', kind: 'ics', url: 'http://localhost:8765/cal.ics' },
  { id: 'a', kind: 'ics', url: ' https://example.com/x ' },
  { id: 'a', kind: 'ics' },
  { id: 5, kind: 'ics', url: U },
  { id: 'g1', kind: 'google', url: 'https://calendar.google.com/x', enabled: true },
  { id: 'g1', kind: 'google', url: 'https://calendar.google.com/x', enabled: 0 },
  { id: 'g1', kind: 'google', url: 'https://calendar.google.com/x', label: null },
  {}
]) add('normalizeFeed', f)
add('feedMetaFrom', { id: 'g1', kind: 'google', label: 'Kelas', enabled: true, url: 'https://secret' })
add('feedMetaFrom', { id: 'g1', kind: 'ics', label: 'x', enabled: false })
add('feedMetaFrom', { id: 'g1', kind: 'ics', label: 'x' })
add('upsertFeedMeta', [{ id: 'moodle', kind: 'moodle', label: 'M', enabled: true }], { id: 'g1', kind: 'google', label: 'K', enabled: true })
add('upsertFeedMeta', [{ id: 'g1', kind: 'google', label: 'K', enabled: true }], { id: 'g1', kind: 'google', label: 'Baru', enabled: false })
add('upsertFeedMeta', null, { id: 'g1', kind: 'ics', label: 'x', enabled: true })
add('upsertFeedMeta', [], { id: 'g1', kind: 'ics', label: 'x', enabled: true })

// ---- feedUrl
for (const v of [1, 60000, 900000, 1200000, 1800000, 3600000, 0, -5, null, 'x', 1e12, 899999, 1800001]) add('clampInterval', v)
for (const v of ['https://x.edu/calendar/export_execute.php?userid=1&authtoken=abc', 'http://h:8080/p?q=1#f', 'https://h:443/p', 'http://h:80/', 'https://User:pw@h/p', 'nonsense', '', 'https://[::1]:3000/a']) add('describeUrl', v)
for (const v of ['https://example.edu/a.ics', ' https://example.edu/a.ics ', '', '   ', null, 5, 'nonsense', 'file:///C:/secrets.ics', 'javascript:alert(1)', 'ftp://x/y', 'HTTPS://EXAMPLE.EDU/A', 'https://example.edu', 'http://h:80/x', 'https://h/a b']) add('validateUrl', v)
for (const o of [
  { refreshMs: 1200000, ok: true }, { refreshMs: 1200000, ok: false, transportFailures: 1 }, { refreshMs: 1200000, ok: false, transportFailures: 2 },
  { refreshMs: 1200000, ok: false, transportFailures: 4 }, { refreshMs: 1200000, ok: false, transportFailures: 5 }, { refreshMs: 1200000, ok: false, transportFailures: 0 },
  { refreshMs: 60000, ok: true }, { refreshMs: 9e9, ok: true }, { refreshMs: null, ok: false, transportFailures: 3 }
]) add('nextRefreshDelayMs', o)
for (const v of [
  'https://x.edu/calendar/export_execute.php?authtoken=1', 'https://x.edu/my/', 'https://x.edu/my', 'https://x.edu/calendar/view.php?view=month',
  'https://x.edu/course/view.php?id=2', 'https://x.edu/mod/assign/view.php?id=3', 'https://x.edu/', 'https://x.edu',
  'https://x.edu/calendar/export_execute.php', 'https://x.edu/other', 'not a url', '', 'https://x.edu/Calendar/EXPORT_EXECUTE.php?authtoken=z',
  'https://x.edu/calendar/export_execute.php?AUTHTOKEN=z'
]) add('inspectMoodleUrl', v)

// ---- hotkey.isValidAccelerator
for (const v of ['', 'Ctrl+Alt+D', 'Super+F12', 'Alt+Space', 'Ctrl+Shift+ArrowUp', 'Ctrl+9', 'D', 'Shift+D', 'Ctrl+', '+D', 'Ctrl+Ctrl+D', 'Ctrl+Alt+Shift+Super+Ctrl+D', 'Ctrl+dd', 'Win+D', 'Ctrl+F25', 'Ctrl+F0', 'Ctrl+F01', 'Ctrl+F1', 'Ctrl+F24', 'Ctrl+Alt+Shift+Super+D', 'Shift+Super+PageDown', 'ctrl+alt+d', null, 5, ['Ctrl+D']]) add('isValidAccelerator', v)

// ---- settings.normalizeSettings
for (const v of [
  null, 'x', 42, [], {}, { language: 'fr', theme: 'neon', restingStyle: 'never' }, { language: 'id', theme: 'auto', restingStyle: 'hover' },
  { refreshMinutes: 1, hoverDelayMs: 99999, keepOverdueDays: -4, offsetX: 99999, monitor: -1 },
  { refreshMinutes: 999 }, { refreshMinutes: 'abc' }, { refreshMinutes: '25' }, { refreshMinutes: null }, { refreshMinutes: 17.5 },
  { dueHorizonHours: null }, { dueHorizonHours: 0 }, { dueHorizonHours: 1000 }, { dueHorizonHours: '48' }, { dueHorizonHours: 2.5 }, { dueHorizonHours: -2.5 },
  { hoverDelayMs: 150.5 }, { hoverDelayMs: '200' }, { hoverDelayMs: true }, { offsetX: -2.5 }, { offsetX: '-300' }, { monitor: 2.5 },
  { thresholdsHours: [1, '6', 24, 6, -1, 'x'] }, { thresholdsHours: [] }, { thresholdsHours: 'x' },
  { toast: false }, { toast: 0 }, { toast: null }, { onboarded: true }, { onboarded: 1 },
  { hotkey: 'Ctrl+Alt+K' }, { hotkey: '' }, { hotkey: 'D' }, { hotkey: 42 }, { hotkey: null },
  {
    feeds: [
      { id: 'a', kind: 'moodle', label: ' BRONE ', enabled: true, url: 'https://secret' },
      { id: 'a', kind: 'google', label: 'dupe' }, { id: 'bad id', kind: 'ics' }, { id: 'b', kind: 'dropbox' },
      { id: 'c', kind: 'ics', enabled: false }, null, 5, { id: 'd', kind: 'ics', label: 'x'.repeat(100) }, { id: 'e', kind: 'ics', label: '   ' },
      { id: 'f', kind: 'ics', label: 42 }, { id: ' g ', kind: 'ics' }
    ]
  },
  { feeds: 'nope' }, { feeds: null }
]) add('normalizeSettings', v)

// =====================================================================
// Task pipeline: merge, done store, personal events, view, planner
// =====================================================================

const H = 3600 * 1000
const D = 24 * H
const NOW = Date.UTC(2026, 9, 22, 12, 0, 0)
const T = (id, title, dueMs, extra = {}) => ({
  id, uid: id.split('::')[0], title, course: null, phase: 'due', due: new Date(dueMs).toISOString(), dueMs,
  allDay: false, description: '', url: null, ...extra
})

// ---- mergeFeedTasks
add('mergeFeedTasks', [
  { feed: { id: 'moodle', kind: 'moodle' }, tasks: [T('a::1', 'A', NOW + H), T('c::3', 'C', NOW + 3 * H)] },
  { feed: { id: 'g1', kind: 'google' }, tasks: [T('b::2', 'B', NOW + 2 * H)] }
])
add('mergeFeedTasks', [
  { feed: { id: 'moodle', kind: 'moodle' }, tasks: [T('x::1', 'Same', NOW + H)] },
  { feed: { id: 'g1', kind: 'google' }, tasks: [T('x::1', 'Same', NOW + H)] }
])
add('mergeFeedTasks', [{ feed: { id: 'f' }, tasks: [T('c::3', 'Tugas', NOW + D), T('b::2', 'Tugas', NOW + D)] }])
add('mergeFeedTasks', [{ feed: { id: 'f' }, tasks: [T('a::1', 'Kuis Bab 4', NOW + D), T('b::2', '  kuis bab 4 ', NOW + D), T('c::3', 'Kuis Bab 4', NOW + 2 * D)] }])
add('mergeFeedTasks', [{ feed: { id: 'f1' }, tasks: [T('a::1', 'Kuis', NOW + D)] }, { feed: { id: 'f2' }, tasks: [T('b::2', 'Kuis', NOW + D)] }])
add('mergeFeedTasks', [{ feed: { id: 'f', kind: '' }, tasks: [T('a::1', 'A', NOW)] }])
add('mergeFeedTasks', [null, {}, { feed: { id: 5 }, tasks: [] }, { feed: { id: 'f' }, tasks: 'nope' }, { feed: { id: 'f' }, tasks: [null, { title: 'no id' }, T('a::1', 'A', NOW)] }])
add('mergeFeedTasks', 'nope')
add('mergeFeedTasks', [])
add('mergeFeedTasks', [{ feed: { id: 'f' }, tasks: [T('z::9', 'Beta', NOW), T('y::8', 'Alpha', NOW), T('w::7', 'Gamma', NOW - D)] }])

// ---- done store (a script of calls on one store)
const tasks3 = [T('a::1', 'A', NOW + D), T('b::2', 'B', NOW + 2 * D), T('c::3', 'C', NOW - D)]
add('doneScript', {}, [{ op: 'isDone', id: 'a::1' }, { op: 'mark', id: 'a::1', iso: '2026-01-01T00:00:00.000Z' }, { op: 'isDone', id: 'a::1' }, { op: 'toJSON' }, { op: 'lastAction' }])
add('doneScript', {}, [{ op: 'mark', id: 'a::1', iso: '2026-01-01T00:00:00.000Z' }, { op: 'mark', id: 'a::1', iso: '2026-02-02T00:00:00.000Z' }, { op: 'toJSON' }])
add('doneScript', { 'a::1': '2026-01-01T00:00:00.000Z' }, [{ op: 'visible', tasks: tasks3 }, { op: 'doneList', tasks: tasks3 }, { op: 'unmark', id: 'a::1' }, { op: 'visible', tasks: tasks3 }, { op: 'lastAction' }, { op: 'unmark', id: 'a::1' }, { op: 'toJSON' }])
add('doneScript', {}, [{ op: 'mark', id: 'a::1', iso: 'x' }, { op: 'undo' }, { op: 'toJSON' }, { op: 'lastAction' }, { op: 'undo' }, { op: 'toJSON' }])
add('doneScript', { 'a::1': 'i', 'b::2': 'j' }, [{ op: 'unmark', id: 'a::1' }, { op: 'lastAction' }])
add('doneScript', { 'live::5': 'i', ['gone-recent::' + (NOW - H)]: 'i', ['gone-old::' + (NOW - 10 * D)]: 'i', garbage: 'i', 'empty::': 'i', 'nan::abc': 'i', ['multi::' + (NOW - H) + '::x']: 'i' },
  [{ op: 'prune', tasks: [T('live::5', 'L', NOW + D)], keepMs: 3 * D, nowMs: NOW }, { op: 'toJSON' }])
add('doneScript', { ['edge::' + (NOW - 3 * D)]: 'i', ['edge2::' + (NOW - 3 * D - 1)]: 'i' }, [{ op: 'prune', tasks: [], keepMs: 3 * D, nowMs: NOW }])

// ---- personal events
const DAYSTR = (offsetDays) => new Date(NOW + offsetDays * D).toISOString().slice(0, 10)
for (const [title, date, time] of [
  ['Rapat', DAYSTR(1), undefined], ['Rapat', DAYSTR(1), ''], ['Rapat', DAYSTR(1), '09:00'], ['Rapat', DAYSTR(1), '00:00'], ['Rapat', DAYSTR(1), '24:00'],
  ['  ', DAYSTR(1), undefined], ['', DAYSTR(1), undefined], [null, DAYSTR(1), undefined], ['Rapat', '', undefined], ['Rapat', '19/09/2026', undefined],
  ['Rapat', undefined, undefined], ['Rapat', DAYSTR(1), '25:99'], ['Rapat', DAYSTR(1), '24:30'], ['Rapat', DAYSTR(1), '9:00'], ['Rapat', DAYSTR(1), 'abc'],
  ['Rapat', DAYSTR(1), '12:60'], ['Rapat', '2026-13-01', undefined], ['Rapat', '2026-00-10', undefined], ['Rapat', '2026-11-00', undefined],
  ['Rapat', '2026-11-32', undefined], ['Rapat', '2026-02-30', undefined], ['Rapat', '2026-02-31', undefined], ['Rapat', '2026-04-31', undefined],
  ['Lama', DAYSTR(200), undefined], ['Basi', DAYSTR(-10), undefined], ['Pas', DAYSTR(-2), '23:59'], ['Jauh', DAYSTR(119), '12:00'], ['Terlalu', DAYSTR(121), '12:00'],
  ['Spring gap NY', '2026-11-01', '01:30'], ['Spring gap EU', '2026-10-25', '01:30'], ['Gap NY', '2026-11-08', '02:30'], [' padded ', ' ' + DAYSTR(1) + ' ', ' 08:30 '],
  ['Emoji \ud83d\ude00', DAYSTR(2), '10:00'], [42, DAYSTR(1), undefined]
]) add('normalizeLocalEvent', { title, date, time }, NOW)
add('localTaskFrom', { id: 'local-x', title: 'Rapat', dueMs: NOW + D, allDay: false })
add('localTaskFrom', { id: 'local-y', title: 'Sehari', dueMs: NOW + D, allDay: true })
add('withLocalTasks', [T('f1', 'Tugas', NOW + 2 * D)], [{ id: 'local-x', title: 'Rapat', dueMs: NOW + D, allDay: false }], NOW)
add('withLocalTasks', [], [{ id: 'local-old', title: 'Basi', dueMs: NOW - 4 * D, allDay: false }], NOW)
add('withLocalTasks', [], [{ id: 'local-old', title: 'Basi', dueMs: NOW - 4 * D, allDay: false }], NOW, 10 * D)
add('withLocalTasks', [T('same', 'Tugas', NOW + D)], [{ id: 'same', title: 'Rapat', dueMs: NOW + D, allDay: false }], NOW)
add('withLocalTasks', [T('f1', 'B', NOW + D)], [{ id: 'l1', title: 'A', dueMs: NOW + D, allDay: false }, { id: 'l2', title: 'C', dueMs: NOW + D, allDay: true }], NOW)
add('withLocalTasks', null, null, NOW)
add('withLocalTasks', [], [{ id: 'x' }, null, { id: 'y', dueMs: 'soon' }], NOW)

// ---- planner
const task = (id, hoursAhead) => ({ id, title: id, dueMs: NOW + hoursAhead * H })
add('planNotifications', [task('a', 48)], [24, 6, 1], NOW)
add('planNotifications', [task('a', 23)], [24, 6, 1], NOW)
add('planNotifications', [task('a', 23)], [24, 6, 1], NOW, { a: [24] })
add('planNotifications', [task('a', 5.5)], [24, 6, 1], NOW, { a: [24] })
add('planNotifications', [task('a', 0.5)], [24, 6, 1], NOW)
add('planNotifications', [task('a', 0.5)], [24, 6, 1], NOW, { a: [24, 6, 1] })
add('planNotifications', [task('late', -1), null, { id: 'x' }, { dueMs: NOW + H }, { id: 'ok', dueMs: 'soon' }], [24, 6, 1], NOW)
add('planNotifications', [task('far', 20), task('near', 2), task('mid', 5)], [24, 6, 1], NOW)
add('planNotifications', [task('a', 1)], [0, -3, null, 'x'], NOW)
add('planNotifications', [task('a', 1)], ['6', 24], NOW)
add('planNotifications', [task('a', 5)], [24, 6, 1], NOW, {}, { a: NOW + H })
add('planNotifications', [task('a', 5)], [24, 6, 1], NOW + 2 * H, {}, { a: NOW + H })
add('planNotifications', [task('a', 5)], [24, 6, 1], NOW, {}, { a: NOW })
add('planNotifications', [task('a', 5)], [24, 6, 1], NOW, {}, { a: 'later' })
add('planNotifications', [task('a', 10), task('b', 10)], [12, 12, 6.5, 2], NOW)
add('planNotifications', null, [1], NOW)
add('planNotifications', [task('a', 1)], 'x', NOW)
add('planNotifications', [task('a', 1)], [24, 6, 1], NOW, { a: 'nope' })
add('recordFired', {}, [{ taskId: 'a', crossed: [24, 6, 1] }])
add('recordFired', { a: [24] }, [{ taskId: 'a', crossed: [6] }])
add('recordFired', { a: [1, 24], b: [6] }, [{ taskId: 'c', crossed: [1, 6] }])
add('recordFired', null, [{ taskId: 'a', crossed: [3, 1, 2] }])
add('recordFired', { a: [24] }, [])
add('pruneFired', { 'live::5': [24], ['recent::' + (NOW - H)]: [1], ['old::' + (NOW - 10 * D)]: [1], garbage: [1], 'empty::': [1] }, [{ id: 'live::5' }], 3 * D, NOW)
add('pruneFired', {}, [], 3 * D, NOW)
add('pruneFired', { ['edge::' + (NOW - 3 * D)]: [1] }, [], 3 * D, NOW)
add('groupNotifications', [1, 2, 3, 4, 5].map((n) => ({ taskId: String(n) })), 3)
add('groupNotifications', [1, 2].map((n) => ({ taskId: String(n) })), 3)
add('groupNotifications', [1, 2, 3, 4].map((n) => ({ taskId: String(n) })))
add('groupNotifications', [], 3)
for (const [kind, now] of [
  ['30m', NOW], ['2h', NOW], ['whenever', NOW], ['', NOW], [null, NOW],
  ['tomorrow', NOW], ['tomorrow', Date.UTC(2026, 2, 7, 20, 0, 0)], ['tomorrow', Date.UTC(2026, 2, 8, 3, 0, 0)], ['tomorrow', Date.UTC(2026, 2, 28, 22, 0, 0)],
  ['tomorrow', Date.UTC(2026, 10, 1, 3, 0, 0)], ['tomorrow', Date.UTC(2026, 9, 24, 23, 30, 0)], ['tomorrow', Date.UTC(2026, 3, 4, 10, 0, 0)],
  ['tomorrow', Date.UTC(2026, 9, 3, 12, 0, 0)], ['tomorrow', Date.UTC(2026, 11, 31, 20, 0, 0)], ['tomorrow', Date.UTC(2026, 1, 28, 20, 0, 0)]
]) add('snoozeUntil', kind, now)
add('isSnoozed', { a: NOW + 1 }, 'a', NOW)
add('isSnoozed', { a: NOW }, 'a', NOW)
add('isSnoozed', {}, 'a', NOW)
add('isSnoozed', null, 'a', NOW)
add('isSnoozed', { a: 'x' }, 'a', NOW)
add('expireSnoozes', { gone: NOW - 1, live: NOW + H }, { gone: [24, 6], live: [24], other: [1] }, NOW)
add('expireSnoozes', { live: NOW + H }, { live: [24] }, NOW)
add('expireSnoozes', {}, { a: [1] }, NOW)
add('expireSnoozes', null, null, NOW)
add('expireSnoozes', { bad: 'x', exact: NOW }, { bad: [1], exact: [2], keep: [3] }, NOW)

// ---- view
const fr = (...tasks) => [{ feed: { id: 'f', kind: 'ics' }, tasks }]
const V = (extra) => ({ feedResults: [], localEvents: [], done: {}, settings: { keepOverdueDays: 3 }, nowMs: NOW, ...extra })
add('buildView', V({ feedResults: fr(T('b::2', 'b', NOW + 10 * H), T('a::1', 'a', NOW + 2 * H), T('x::3', 'x', NOW - 5 * H), T('y::4', 'y', NOW - H)) }))
add('buildView', V({ feedResults: fr(T('late::5', 'late', NOW - 2 * H)) }))
add('buildView', V({}))
add('buildView', V({ feedResults: fr(T('a::1', 'a', NOW + 2 * H), T('b::2', 'b', NOW + 3 * H)), done: { 'a::1': '2026-01-01T00:00:00.000Z' } }))
add('buildView', V({ feedResults: fr(T('old::1', 'old', NOW - 4 * D), T('recent::2', 'recent', NOW - 2 * D)) }))
add('buildView', V({ feedResults: fr(T('a::1', 'a', NOW + 5 * H)), localEvents: [{ id: 'local-1', title: 'Rapat', dueMs: NOW + H, allDay: false }] }))
add('buildView', V({ feedResults: fr(T('old::1', 'old', NOW - 5 * D)), settings: { keepOverdueDays: 7 } }))
add('buildView', V({ feedResults: fr(T('t1::1', 'same', NOW - 2 * H), T('t2::2', 'same2', NOW - 2 * H), T('t3::3', 'later', NOW - H)) }))
add('buildView', V({ localEvents: [{ id: 'local-old', title: 'Basi', dueMs: NOW - 4 * D, allDay: false }, { id: 'local-ok', title: 'Ok', dueMs: NOW - D, allDay: true }], settings: { keepOverdueDays: 0 } }))
const base = { nowMs: NOW, dueHorizonHours: 72 }
const heroNear = T('n::1', 'n', NOW + 10 * H)
const heroFar = T('f::2', 'f', NOW + 10 * D)
for (const [style, hero] of [
  ['hover', heroNear], ['always', null], ['always', heroFar], ['when-due', heroNear], ['when-due', heroFar], ['when-due', null],
  ['when-due', T('late::3', 'late', NOW - H)], ['when-due', T('edge::4', 'edge', NOW + 72 * H)], ['when-due', T('edge2::5', 'edge2', NOW + 72 * H + 1)], ['weird', heroNear]
]) add('restModeFor', { ...base, restingStyle: style, hero })
// ---- personal-event window boundaries. The clock is given as a local wall time
// plus an offset, because the exact instant depends on the machine zone.
const wallNow = (wall, plusMs) => ({ $localWall: wall, plusMs })
for (const [label, now, date, time] of [
  ['exactly at the past limit', wallNow('2026-10-19T12:00:00', 3 * D), '2026-10-19', '12:00'],
  ['one millisecond past the limit', wallNow('2026-10-19T12:00:00', 3 * D + 1), '2026-10-19', '12:00'],
  ['exactly at the far limit', wallNow('2027-02-19T12:00:00', -120 * D), '2027-02-19', '12:00'],
  ['one millisecond beyond the far limit', wallNow('2027-02-19T12:00:00', -120 * D - 1), '2027-02-19', '12:00'],
  ['all-day at the past limit', wallNow('2026-10-19T23:59:00', 3 * D), '2026-10-19', ''],
  ['all-day one millisecond past', wallNow('2026-10-19T23:59:00', 3 * D + 1), '2026-10-19', '']
]) add('normalizeLocalEvent', { title: `Edge: ${label}`, date, time }, now)
// =====================================================================
// Translations, countdown text, formatting
// =====================================================================

// ---- t: every key in both languages (and an unknown language) with all the usual variables
const ALL_VARS = { n: 3, time: '12:03', title: 'Tugas 1', value: '5h', course: 'CS101', left: '2h 14m', next: 'Quiz 3 (Sat 00:31)', v: '0.1.0', msg: 'x' }
const fnKeys = new Set(Object.entries(STRINGS.en).filter(([, v]) => typeof v === 'function').map(([k]) => k))
for (const lang of ['en', 'id', 'fr']) {
  for (const key of Object.keys(STRINGS.en)) {
    add('t', lang, key, ALL_VARS)
    if (!fnKeys.has(key)) add('t', lang, key, {})
  }
}
add('t', 'en', 'no.such.key', ALL_VARS)
add('t', 'id', 'no.such.key', {})
for (const lang of ['en', 'id']) {
  for (const n of [0, 1, 2, 11]) add('t', lang, 'feeds.test.ok', { n, next: '' })
  add('t', lang, 'feeds.test.ok', { n: 1, next: 'Tugas (Besok 09:00)' })
  add('t', lang, 'toast.body', { course: '', left: '1h' })
  add('t', lang, 'toast.body', { course: 'MA201', left: '1h' })
  add('t', lang, 'toast.summary.title', { n: 1 })
  add('t', lang, 'section.done', { n: 0 })
  add('t', lang, 'footer.synced', { time: '{time}' })
}
for (const [s, n] of [['en', 'id-ID'], ['id', 'en-US'], ['auto', 'id-ID'], ['auto', 'ID'], ['auto', 'en-GB'], ['auto', undefined], ['auto', ''], ['auto', null], ['fr', 'id'], [null, 'id-ID'], ['id', 'id']]) add('resolveLanguage', s, n)

// ---- urgency and countdown text
const MIN = 60 * 1000
const CN = Date.UTC(2026, 9, 22, 10, 0, 0)
for (const d of [-1, 0, 1, D, D + 1, -D, 90 * D]) add('urgencyOf', CN + d, CN)
const deltas = [0, 1, 999, 1000, 59 * 1000, MIN - 1, MIN, MIN + 1, 5 * MIN, 59 * MIN, 59 * MIN + 59 * 1000, 60 * MIN - 1, 60 * MIN, 61 * MIN, 2 * H + 14 * MIN,
  23 * H + 59 * MIN, 24 * H - 1, 24 * H, 24 * H + 1, 25 * H + 30 * MIN, 2 * D + 3 * H, 10 * D + 23 * H, 400 * D]
for (const lang of ['en', 'id']) {
  for (const d of deltas) {
    for (const sign of [1, -1]) {
      add('compactCountdown', CN + sign * d, CN, lang, {})
      add('compactCountdown', CN + sign * d, CN, lang, { seconds: true })
    }
  }
}

// ---- due labels: days around now, every month, every weekday, around daylight-saving changes
// A wall time on a calendar day counted from 2026-10-22, resolved in each machine zone by both sides.
const dueAt = (days, hh = 14, mm = 5) => ({ $localWall: new Date(Date.UTC(2026, 9, 22 + days, hh, mm, 0)).toISOString().slice(0, 19), plusMs: 0 })
for (const lang of ['en', 'id']) {
  for (const days of [-3, -1, 0, 1, 2, 3, 6, 7, 8, 20, 80, 200, 400]) for (const [hh, mm] of [[0, 0], [9, 5], [14, 5], [23, 59]]) add('dueLabel', dueAt(days, hh, mm), CN, lang)
  for (let m = 0; m < 12; m++) add('dueLabel', Date.UTC(2027, m, 15, 12, 0, 0), CN, lang)
  for (let w = 0; w < 7; w++) add('dueLabel', dueAt(2 + w, 9, 5), CN, lang)
  for (const around of [Date.UTC(2026, 2, 8, 6, 0, 0), Date.UTC(2026, 2, 29, 0, 30, 0), Date.UTC(2026, 9, 25, 0, 30, 0), Date.UTC(2026, 10, 1, 5, 30, 0), Date.UTC(2026, 9, 3, 15, 0, 0)]) {
    for (const k of [0, 1, 2, 3]) add('dueLabel', around + k * D, around - D / 2, lang)
  }
}
for (const [last, now, mins] of [[NaN, CN, 20], [CN - 20 * MIN, CN, 20], [CN - 21 * MIN, CN, 20], [CN - 22 * MIN, CN, 20], [CN - 10 * MIN, CN, 5], [CN - 10 * MIN, CN, 0], [CN - 10 * MIN, CN, 'x'], [CN - 30 * MIN, CN, undefined], [null, CN, 20]]) add('isDataStale', last, now, mins)
for (const [due, live] of [[CN + 10 * MIN, true], [CN + 5 * H, true], [CN + 10 * MIN, false], [CN - 10 * MIN, true], [CN + H, true], [CN + H - 1, true], [CN, true]]) add('nextTickMs', due, CN, { live })

// ---- format helpers
for (const [text, max] of [['short', 10], ['abcdefghij', 5], [null, 5], ['  padded  ', 20], ['exact', 5], ['sixsix', 5], ['a b c d e f', 6], ['abc  def', 5], ['\u00e9\u00e8\u00ea\u00eb\u00ec', 3], ['ab\ud83d\ude00cd', 5], ['', 3], ['xyz', 1], ['xyz', 0], [12345, 3]]) add('truncate', text, max)
const P = (course, title = 'T') => ({ course, title })
for (const [task, max] of [
  [P('CS101 Algorithms'), 14], [P('ma 201', 'Quiz'), 14], [P('Desain dan Analisis Algoritma', 'Tugas 1'), 14], [P(null, 'Rapat'), 14], [null, 14], [P('Basis Data 2', 'x'), 20],
  [P('CS-101'), 14], [P('CS 101'), 14], [P('CS - 101'), 14], [P('CS101a x'), 14], [P('CS101ab'), 14], [P('CS1'), 14], [P('C101'), 14], [P('ABCDEFG101'), 14],
  [P('ABCDEF1234'), 14], [P('ABC12345'), 14], [P('abc123b'), 14], [P('AB 12 extra'), 14], [P('AB\u00a0123'), 14], [P('  CS101  '), 14], [P('', 'Judul yang sangat panjang sekali'), 14],
  [P('Ruang  Kelas'), 6], [P('abc12345'), 14], [P('abc1234x1'), 14], [P('ab1'), 14], [P('abc-12'), 14], [P('abc- 12'), 14], [P(undefined, 'Tanpa kursus'), 20], [{ title: 'only title' }, 9], [P('CS101'), undefined]
]) add('pillLabel', task, max)
const SA = CN
for (const lang of ['en', 'id']) {
  for (const feeds of [
    {}, null, { a: { syncing: true } }, { a: { fetchedAtMs: SA } }, { a: { fetchedAtMs: SA, error: { code: 'NETWORK', transport: true } } },
    { a: { error: { code: 'UNAUTHORIZED', transport: false } } }, { a: {} }, { a: { error: { code: 'NETWORK', transport: true } } },
    { a: { fetchedAtMs: SA }, b: { fetchedAtMs: SA + 5 * MIN } }, { a: { fetchedAtMs: SA }, b: { error: { code: 'NOT_CALENDAR', transport: false } } },
    { a: { fetchedAtMs: SA, syncing: false }, b: { syncing: true } }, { a: { fetchedAtMs: SA + 2 * H }, b: { fetchedAtMs: SA, error: { code: 'TIMEOUT', transport: true } } },
    { a: { fetchedAtMs: null }, b: { fetchedAtMs: SA } }, { a: { error: { code: 'HTTP_ERROR', transport: false } }, b: { error: { code: 'TOO_LARGE', transport: false } } }
  ]) add('syncSummary', feeds, lang)
}