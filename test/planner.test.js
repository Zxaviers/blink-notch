import test from 'node:test'
import assert from 'node:assert/strict'
import {
  planNotifications, recordFired, pruneFired, groupNotifications,
  snoozeUntil, isSnoozed, expireSnoozes
} from '../src/lib/planner.js'

const H = 3600 * 1000
const NOW = 1_000_000_000_000
const task = (id, hoursAhead) => ({ id, title: id, dueMs: NOW + hoursAhead * H })

test('nothing is announced while every threshold is still ahead', () => {
  assert.deepEqual(planNotifications([task('a', 48)], [24, 6, 1], NOW), [])
})

test('crossing a threshold announces it once', () => {
  const plans = planNotifications([task('a', 23)], [24, 6, 1], NOW)
  assert.equal(plans.length, 1)
  assert.equal(plans[0].thresholdHours, 24)
  const fired = recordFired({}, plans)
  assert.deepEqual(planNotifications([task('a', 22)], [24, 6, 1], NOW, fired), [])
})

test('the next threshold fires later even after an earlier one fired', () => {
  const fired = { a: [24] }
  const plans = planNotifications([task('a', 5.5)], [24, 6, 1], NOW, fired)
  assert.equal(plans.length, 1)
  assert.equal(plans[0].thresholdHours, 6)
})

test('catch-up: several crossed thresholds announce only the tightest and record all', () => {
  const plans = planNotifications([task('a', 0.5)], [24, 6, 1], NOW)
  assert.equal(plans.length, 1)
  assert.equal(plans[0].thresholdHours, 1)
  assert.deepEqual(plans[0].crossed, [24, 6, 1])
  const fired = recordFired({}, plans)
  assert.deepEqual(fired.a, [24, 6, 1])
  assert.deepEqual(planNotifications([task('a', 0.4)], [24, 6, 1], NOW, fired), [])
})

test('overdue and malformed tasks are skipped', () => {
  const tasks = [task('late', -1), null, { id: 'x' }, { dueMs: NOW + H }]
  assert.deepEqual(planNotifications(tasks, [24, 6, 1], NOW), [])
})

test('plans are ordered most urgent first', () => {
  const plans = planNotifications([task('far', 20), task('near', 2)], [24, 6, 1], NOW)
  assert.deepEqual(plans.map((p) => p.taskId), ['near', 'far'])
})

test('invalid thresholds are ignored and inputs are not mutated', () => {
  assert.deepEqual(planNotifications([task('a', 1)], [0, -3, NaN], NOW), [])
  const fired = { a: [24] }
  recordFired(fired, [{ taskId: 'a', crossed: [6] }])
  assert.deepEqual(fired, { a: [24] })
  assert.deepEqual(planNotifications(null, [1], NOW), [])
})

test('pruneFired keeps live tasks and recent leavers, drops expired ones', () => {
  const keep = 3 * 24 * H
  const live = { id: 'live::5', title: 'l', dueMs: NOW + H }
  const fired = {
    'live::5': [24],
    [`gone-recent::${NOW - H}`]: [1],
    [`gone-old::${NOW - 10 * 24 * H}`]: [1],
    garbage: [1]
  }
  const next = pruneFired(fired, [live], keep, NOW)
  assert.deepEqual(Object.keys(next).sort(), ['live::5', `gone-recent::${NOW - H}`].sort())
})

test('groupNotifications caps individual toasts and counts the rest', () => {
  const plans = [1, 2, 3, 4, 5].map((n) => ({ taskId: String(n) }))
  const grouped = groupNotifications(plans, 3)
  assert.equal(grouped.individual.length, 3)
  assert.equal(grouped.extra, 2)
  assert.equal(groupNotifications(plans.slice(0, 2), 3).extra, 0)
})

test('a snoozed task stays quiet until the snooze ends', () => {
  const snoozed = { a: NOW + H }
  assert.deepEqual(planNotifications([task('a', 5)], [24, 6, 1], NOW, {}, snoozed), [])
  assert.equal(planNotifications([task('a', 5)], [24, 6, 1], NOW + 2 * H, {}, snoozed).length, 1)
})

test('snoozeUntil: 30 minutes, 2 hours, tomorrow 08:00 local, unknown', () => {
  const now = new Date('2026-10-22T20:15:00').getTime()
  assert.equal(snoozeUntil('30m', now), now + 30 * 60 * 1000)
  assert.equal(snoozeUntil('2h', now), now + 2 * H)
  assert.equal(snoozeUntil('tomorrow', now), new Date('2026-10-23T08:00:00').getTime())
  assert.equal(snoozeUntil('whenever', now), null)
})

test('isSnoozed only while the snooze is in the future', () => {
  assert.equal(isSnoozed({ a: NOW + 1 }, 'a', NOW), true)
  assert.equal(isSnoozed({ a: NOW }, 'a', NOW), false)
  assert.equal(isSnoozed({}, 'a', NOW), false)
  assert.equal(isSnoozed(undefined, 'a', NOW), false)
})

test('expired snoozes are dropped and their fired thresholds cleared so they announce again', () => {
  const snoozed = { gone: NOW - 1, live: NOW + H }
  const fired = { gone: [24, 6], live: [24], other: [1] }
  const out = expireSnoozes(snoozed, fired, NOW)
  assert.deepEqual(out.snoozed, { live: NOW + H })
  assert.deepEqual(out.fired, { live: [24], other: [1] })
  assert.equal(out.changed, true)
  assert.deepEqual(snoozed, { gone: NOW - 1, live: NOW + H }, 'input untouched')
  assert.equal(expireSnoozes({ live: NOW + H }, fired, NOW).changed, false)
})