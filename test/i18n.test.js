import test from 'node:test'
import assert from 'node:assert/strict'
import { STRINGS, createT, resolveLanguage } from '../src/lib/i18n.js'

test('English and Indonesian define exactly the same keys', () => {
  const en = Object.keys(STRINGS.en).sort()
  const id = Object.keys(STRINGS.id).sort()
  assert.deepEqual(id.filter((k) => !en.includes(k)), [], 'keys only in id')
  assert.deepEqual(en.filter((k) => !id.includes(k)), [], 'keys only in en')
})

test('placeholders match between languages', () => {
  const holes = (v) => (typeof v === 'string' ? (v.match(/\{\w+\}/g) ?? []).sort().join() : 'fn')
  for (const key of Object.keys(STRINGS.en)) {
    assert.equal(holes(STRINGS.id[key]), holes(STRINGS.en[key]), key)
  }
})

test('t fills placeholders, supports plural functions, and returns the key when unknown', () => {
  const en = createT('en')
  assert.equal(en('section.done', { n: 3 }), 'Done (3)')
  assert.equal(en('toast.body', { course: 'CS101', left: '1 hour' }), '[CS101] Due in 1 hour.')
  assert.equal(en('no.such.key'), 'no.such.key')
  assert.equal(createT('id')('section.done', { n: 2 }), 'Selesai (2)')
})

test('language resolution', () => {
  assert.equal(resolveLanguage('en', 'id-ID'), 'en')
  assert.equal(resolveLanguage('id', 'en-US'), 'id')
  assert.equal(resolveLanguage('auto', 'id-ID'), 'id')
  assert.equal(resolveLanguage('auto', 'en-GB'), 'en')
  assert.equal(resolveLanguage('auto', undefined), 'en')
})
