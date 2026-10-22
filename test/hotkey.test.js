import test from 'node:test'
import assert from 'node:assert/strict'
import {
  DEFAULT_HOTKEY, acceleratorFromEvent, isValidAccelerator, nextSelection, keyAction
} from '../src/lib/hotkey.js'

test('default hotkey is valid', () => {
  assert.equal(isValidAccelerator(DEFAULT_HOTKEY), true)
})

test('key press becomes an accelerator with modifiers in a fixed order', () => {
  assert.deepEqual(acceleratorFromEvent({ code: 'KeyD', ctrlKey: true, altKey: true }), { accelerator: 'Ctrl+Alt+D' })
  assert.deepEqual(acceleratorFromEvent({ code: 'F8', metaKey: true }), { accelerator: 'Super+F8' })
  assert.deepEqual(acceleratorFromEvent({ code: 'Digit1', ctrlKey: true, shiftKey: true, altKey: true }), { accelerator: 'Ctrl+Alt+Shift+1' })
  assert.deepEqual(acceleratorFromEvent({ code: 'Space', altKey: true }), { accelerator: 'Alt+Space' })
})

test('a bare modifier press is only partial', () => {
  assert.deepEqual(acceleratorFromEvent({ code: 'ControlLeft', ctrlKey: true }), { error: 'PARTIAL' })
  assert.deepEqual(acceleratorFromEvent({ code: 'MetaLeft', metaKey: true }), { error: 'PARTIAL' })
})

test('shortcuts need Ctrl, Alt or Windows and a supported key', () => {
  assert.deepEqual(acceleratorFromEvent({ code: 'KeyD' }), { error: 'MODIFIER' })
  assert.deepEqual(acceleratorFromEvent({ code: 'KeyD', shiftKey: true }), { error: 'MODIFIER' })
  assert.deepEqual(acceleratorFromEvent({ code: 'Semicolon', ctrlKey: true }), { error: 'KEY' })
  assert.deepEqual(acceleratorFromEvent({ code: undefined, ctrlKey: true }), { error: 'KEY' })
})

test('isValidAccelerator accepts good forms, empty (off), and rejects the rest', () => {
  for (const ok of ['', 'Ctrl+Alt+D', 'Super+F12', 'Alt+Space', 'Ctrl+Shift+ArrowUp', 'Ctrl+9']) {
    assert.equal(isValidAccelerator(ok), true, ok)
  }
  for (const bad of ['D', 'Shift+D', 'Ctrl+', '+D', 'Ctrl+Ctrl+D', 'Ctrl+Alt+Shift+Super+Ctrl+D', 'Ctrl+dd', 'Win+D', 'Ctrl+F25', null, 5]) {
    assert.equal(isValidAccelerator(bad), false, String(bad))
  }
})

test('arrow keys move the selection and stop at the ends', () => {
  const ids = ['a', 'b', 'c']
  assert.equal(nextSelection(ids, 'a', 'ArrowDown'), 'b')
  assert.equal(nextSelection(ids, 'c', 'ArrowDown'), 'c')
  assert.equal(nextSelection(ids, 'a', 'ArrowUp'), 'a')
  assert.equal(nextSelection(ids, 'c', 'ArrowUp'), 'b')
  assert.equal(nextSelection(ids, 'b', 'Home'), 'a')
  assert.equal(nextSelection(ids, 'b', 'End'), 'c')
})

test('with nothing selected Down starts at the top and Up at the bottom', () => {
  const ids = ['a', 'b', 'c']
  assert.equal(nextSelection(ids, null, 'ArrowDown'), 'a')
  assert.equal(nextSelection(ids, 'gone', 'ArrowUp'), 'c')
  assert.equal(nextSelection([], 'a', 'ArrowDown'), null)
})

test('key actions in the panel', () => {
  assert.deepEqual(keyAction('ArrowDown'), { type: 'move', key: 'ArrowDown' })
  assert.deepEqual(keyAction('Enter'), { type: 'open' })
  assert.deepEqual(keyAction('d'), { type: 'done' })
  assert.deepEqual(keyAction(' '), { type: 'done' })
  assert.deepEqual(keyAction('S'), { type: 'snooze-menu' })
  assert.deepEqual(keyAction('Escape'), { type: 'close' })
  assert.equal(keyAction('x'), null)
})

test('while the snooze chooser is open, 1 to 3 pick a duration and Esc only closes the chooser', () => {
  const mode = { snoozing: true }
  assert.deepEqual(keyAction('1', mode), { type: 'snooze', kind: '30m' })
  assert.deepEqual(keyAction('2', mode), { type: 'snooze', kind: '2h' })
  assert.deepEqual(keyAction('3', mode), { type: 'snooze', kind: 'tomorrow' })
  assert.deepEqual(keyAction('Escape', mode), { type: 'snooze-cancel' })
  assert.equal(keyAction('d', mode), null)
  assert.equal(keyAction('4', mode), null)
})
