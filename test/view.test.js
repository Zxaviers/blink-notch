import test from 'node:test'
import assert from 'node:assert/strict'
import { buildView, restModeFor } from '../src/lib/view.js'
import { DAY_MS, HOUR_MS } from '../src/lib/countdown.js'

const NOW = 1_000_000_000_000
const task = (id, hoursAhead, extra = {}) => ({
  id, uid: id, title: id, course: null, dueMs: NOW + hoursAhead * HOUR_MS, ...extra
})
const settings = { keepOverdueDays: 3 }
const feed = (...tasks) => [{ feed: { id: 'f', kind: 'ics' }, tasks }]

test('upcoming is nearest-first, overdue is most-recently-missed first and separate', () => {
  const view = buildView({
    feedResults: feed(task('b', 10), task('a', 2), task('x', -5), task('y', -1)),
    localEvents: [], done: {}, settings, nowMs: NOW
  })
  assert.deepEqual(view.upcoming.map((t) => t.id), ['a', 'b'])
  assert.deepEqual(view.overdue.map((t) => t.id), ['y', 'x'])
  assert.equal(view.hero.id, 'a')
})

test('hero falls back to the latest overdue item when nothing is upcoming', () => {
  const view = buildView({ feedResults: feed(task('late', -2)), localEvents: [], done: {}, settings, nowMs: NOW })
  assert.equal(view.hero.id, 'late')
  assert.equal(buildView({ feedResults: [], localEvents: [], done: {}, settings, nowMs: NOW }).hero, null)
})

test('done tasks leave the lists and appear in doneList', () => {
  const view = buildView({
    feedResults: feed(task('a', 2), task('b', 3)),
    localEvents: [], done: { a: '2026-01-01T00:00:00Z' }, settings, nowMs: NOW
  })
  assert.deepEqual(view.upcoming.map((t) => t.id), ['b'])
  assert.deepEqual(view.doneList.map((t) => t.id), ['a'])
})

test('overdue items older than the keep window are dropped', () => {
  const view = buildView({
    feedResults: feed(task('old', -4 * 24), task('recent', -2 * 24)),
    localEvents: [], done: {}, settings, nowMs: NOW
  })
  assert.deepEqual(view.overdue.map((t) => t.id), ['recent'])
})

test('personal events merge into the same list', () => {
  const view = buildView({
    feedResults: feed(task('a', 5)),
    localEvents: [{ id: 'local-1', title: 'Rapat', dueMs: NOW + HOUR_MS, allDay: false }],
    done: {}, settings, nowMs: NOW
  })
  assert.deepEqual(view.upcoming.map((t) => t.id), ['local-1', 'a'])
  assert.equal(view.upcoming[0].source, 'local')
})

test('resting mode follows the setting and the hero', () => {
  const near = task('n', 10)
  const far = task('f', 10 * 24)
  const base = { nowMs: NOW, dueHorizonHours: 72 }
  assert.equal(restModeFor({ ...base, restingStyle: 'hover', hero: near }), 'hot')
  assert.equal(restModeFor({ ...base, restingStyle: 'always', hero: null }), 'full')
  assert.equal(restModeFor({ ...base, restingStyle: 'when-due', hero: near }), 'full')
  assert.equal(restModeFor({ ...base, restingStyle: 'when-due', hero: far }), 'mini')
  assert.equal(restModeFor({ ...base, restingStyle: 'when-due', hero: null }), 'mini')
  assert.equal(restModeFor({ ...base, restingStyle: 'when-due', hero: task('late', -1) }), 'full')
  assert.ok(DAY_MS > 0)
})
