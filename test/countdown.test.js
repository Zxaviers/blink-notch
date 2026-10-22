import test from 'node:test'
import assert from 'node:assert/strict'
import { createT } from '../src/lib/i18n.js'
import {
  urgencyOf, compactCountdown, dueLabel, isDataStale, nextTickMs,
  MINUTE_MS, HOUR_MS, DAY_MS
} from '../src/lib/countdown.js'

const en = createT('en')
const id = createT('id')
const NOW = new Date('2026-10-22T10:00:00').getTime()

test('urgency buckets: overdue, soon within 24h, later beyond', () => {
  assert.equal(urgencyOf(NOW - 1, NOW), 'overdue')
  assert.equal(urgencyOf(NOW + HOUR_MS, NOW), 'soon')
  assert.equal(urgencyOf(NOW + DAY_MS, NOW), 'soon')
  assert.equal(urgencyOf(NOW + DAY_MS + 1, NOW), 'later')
})

test('upcoming countdown uses days+hours, hours+minutes, or minutes', () => {
  assert.equal(compactCountdown(NOW + 3 * DAY_MS + 2 * HOUR_MS, NOW, en), '3d 2h')
  assert.equal(compactCountdown(NOW + 2 * HOUR_MS + 14 * MINUTE_MS, NOW, en), '2h 14m')
  assert.equal(compactCountdown(NOW + 42 * MINUTE_MS, NOW, en), '42m')
})

test('the last hour can show live mm:ss', () => {
  assert.equal(compactCountdown(NOW + 42 * MINUTE_MS + 7000, NOW, en, { seconds: true }), '42:07')
  // Outside the last hour seconds are ignored.
  assert.equal(compactCountdown(NOW + 2 * HOUR_MS, NOW, en, { seconds: true }), '2h 0m')
})

test('overdue always carries a word, never a bare number', () => {
  assert.equal(compactCountdown(NOW - 5 * MINUTE_MS, NOW, en), 'Overdue 5m')
  assert.equal(compactCountdown(NOW - 3 * HOUR_MS, NOW, en), 'Overdue 3h')
  assert.equal(compactCountdown(NOW - 2 * DAY_MS, NOW, en), 'Overdue 2d')
  assert.equal(compactCountdown(NOW - 3 * HOUR_MS, NOW, id), 'Terlewat 3j')
})

test('due labels: today, tomorrow, weekday, date', () => {
  const at = (days, hh = 14) => new Date(`2026-10-${22 + days}T${String(hh).padStart(2, '0')}:00:00`).getTime()
  assert.equal(dueLabel(at(0), NOW, 'en', en), 'Today 14:00')
  assert.equal(dueLabel(at(1, 9), NOW, 'en', en), 'Tomorrow 09:00')
  assert.equal(dueLabel(at(1, 9), NOW, 'id', id), 'Besok 09:00')
  assert.match(dueLabel(at(3), NOW, 'en', en), /14:00$/)
  assert.doesNotMatch(dueLabel(at(8), NOW, 'en', en), /:/)
})

test('data staleness allows one refresh interval plus a minute', () => {
  assert.equal(isDataStale(NaN, NOW), true)
  assert.equal(isDataStale(NOW - 20 * MINUTE_MS, NOW, 20), false)
  assert.equal(isDataStale(NOW - 22 * MINUTE_MS, NOW, 20), true)
})

test('tick interval is one second only inside the last hour while live', () => {
  assert.equal(nextTickMs(NOW + 10 * MINUTE_MS, NOW, { live: true }), 1000)
  assert.equal(nextTickMs(NOW + 5 * HOUR_MS, NOW, { live: true }), 15000)
  assert.equal(nextTickMs(NOW + 10 * MINUTE_MS, NOW, { live: false }), MINUTE_MS)
})
