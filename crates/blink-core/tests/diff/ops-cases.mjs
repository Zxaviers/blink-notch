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
