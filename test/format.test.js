import test from 'node:test'
import assert from 'node:assert/strict'
import { truncate, pillLabel, syncSummary } from '../src/lib/format.js'
import { createT } from '../src/lib/i18n.js'

const t = createT('en')

test('truncate adds an ellipsis only when needed', () => {
  assert.equal(truncate('short', 10), 'short')
  assert.equal(truncate('abcdefghij', 5), 'abcd…')
  assert.equal(truncate(null, 5), '')
})

test('pill label prefers a course code, then the course name, then the title', () => {
  assert.equal(pillLabel({ course: 'CS101 Algorithms', title: 'PS4' }), 'CS101')
  assert.equal(pillLabel({ course: 'ma 201', title: 'Quiz' }), 'MA201')
  assert.equal(pillLabel({ course: 'Desain dan Analisis Algoritma', title: 'Tugas 1' }, 14), 'Desain dan An…')
  assert.equal(pillLabel({ course: null, title: 'Rapat' }), 'Rapat')
  assert.equal(pillLabel(null), '')
  // "Basis Data 2" is not a course code.
  assert.equal(pillLabel({ course: 'Basis Data 2', title: 'x' }, 20), 'Basis Data 2')
})

const at = new Date('2026-10-22T12:03:00').getTime()

test('sync summary: none, syncing, ok, offline, error, never', () => {
  assert.equal(syncSummary({}, t, 'en').kind, 'none')
  assert.equal(syncSummary({ a: { syncing: true } }, t, 'en').kind, 'syncing')
  assert.deepEqual(syncSummary({ a: { fetchedAtMs: at } }, t, 'en'), { kind: 'ok', text: 'Synced 12:03' })
  assert.deepEqual(
    syncSummary({ a: { fetchedAtMs: at, error: { code: 'NETWORK', transport: true } } }, t, 'en'),
    { kind: 'offline', text: 'Offline · last synced 12:03' }
  )
  assert.deepEqual(
    syncSummary({ a: { error: { code: 'UNAUTHORIZED', transport: false } } }, t, 'en').kind,
    'error'
  )
  assert.equal(syncSummary({ a: {} }, t, 'en').kind, 'never')
  assert.equal(syncSummary({ a: { error: { code: 'NETWORK', transport: true } } }, t, 'en').text, 'Not synced yet')
})
