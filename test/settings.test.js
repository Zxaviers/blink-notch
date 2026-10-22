import test from 'node:test'
import assert from 'node:assert/strict'
import { normalizeSettings, DEFAULT_SETTINGS } from '../src/lib/settings.js'
import { nextRefreshDelayMs, MINUTE_MS, inspectMoodleUrl } from '../src/lib/feedUrl.js'

test('garbage input falls back to defaults', () => {
  for (const raw of [null, undefined, 'x', 42, []]) {
    assert.deepEqual(normalizeSettings(raw), { ...DEFAULT_SETTINGS, thresholdsHours: [24, 6, 1], feeds: [] })
  }
})

test('enum fields reject unknown values', () => {
  const s = normalizeSettings({ language: 'fr', theme: 'neon', restingStyle: 'never' })
  assert.equal(s.language, 'auto')
  assert.equal(s.theme, 'dark')
  assert.equal(s.restingStyle, 'always')
})

test('numbers are clamped into sane ranges', () => {
  const s = normalizeSettings({ refreshMinutes: 1, hoverDelayMs: 99999, keepOverdueDays: -4, offsetX: 99999, monitor: -1 })
  assert.equal(s.refreshMinutes, 15)
  assert.equal(s.hoverDelayMs, 1000)
  assert.equal(s.keepOverdueDays, 0)
  assert.equal(s.offsetX, 2000)
  assert.equal(s.monitor, 0)
  assert.equal(normalizeSettings({ refreshMinutes: 999 }).refreshMinutes, 30)
  assert.equal(normalizeSettings({ refreshMinutes: 'abc' }).refreshMinutes, 20)
})

test('thresholds are cleaned, sorted and deduplicated', () => {
  assert.deepEqual(normalizeSettings({ thresholdsHours: [1, '6', 24, 6, -1, 'x'] }).thresholdsHours, [24, 6, 1])
  assert.deepEqual(normalizeSettings({ thresholdsHours: [] }).thresholdsHours, [24, 6, 1])
})

test('feed rows are validated, deduplicated and never carry a url', () => {
  const s = normalizeSettings({
    feeds: [
      { id: 'a', kind: 'moodle', label: ' BRONE ', enabled: true, url: 'https://secret' },
      { id: 'a', kind: 'google', label: 'dupe' },
      { id: 'bad id', kind: 'ics' },
      { id: 'b', kind: 'dropbox' },
      { id: 'c', kind: 'ics', enabled: false },
      null
    ]
  })
  assert.deepEqual(s.feeds, [
    { id: 'a', kind: 'moodle', label: 'BRONE', enabled: true },
    { id: 'c', kind: 'ics', label: 'c', enabled: false }
  ])
})

test('a successful refresh waits the configured interval', () => {
  assert.equal(nextRefreshDelayMs({ refreshMs: 20 * MINUTE_MS, ok: true }), 20 * MINUTE_MS)
})

test('offline failures walk the retry ladder then settle at the interval', () => {
  const at = (n) => nextRefreshDelayMs({ refreshMs: 20 * MINUTE_MS, ok: false, transportFailures: n })
  assert.deepEqual([1, 2, 3, 4].map(at), [1, 2, 4, 8].map((m) => m * MINUTE_MS))
  assert.equal(at(5), 20 * MINUTE_MS)
})

test('moodle url diagnostics return hint codes', () => {
  assert.equal(inspectMoodleUrl('https://x.edu/calendar/export_execute.php?authtoken=1').ok, true)
  assert.equal(inspectMoodleUrl('https://x.edu/my/').hint, 'moodle_dashboard')
  assert.equal(inspectMoodleUrl('https://x.edu/calendar/export_execute.php').hint, 'moodle_missing_token')
  assert.equal(inspectMoodleUrl('https://x.edu/other').hint, 'moodle_not_export')
  assert.equal(inspectMoodleUrl('not a url').hint, null)
})
