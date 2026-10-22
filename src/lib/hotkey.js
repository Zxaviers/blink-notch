/**
 * hotkey.js
 * ---------------------------------------------------------------------------
 * Global hotkey helpers: build an accelerator string from a key press (for the
 * settings "Change" field), validate one, and map keys inside the open panel
 * to actions. Pure.
 *
 * Accelerators use the form the Rust side parses: "Ctrl+Alt+D", "Super+F8".
 * A shortcut must include Ctrl, Alt or Windows (Super); Shift alone would
 * hijack ordinary typing system-wide.
 */

export const DEFAULT_HOTKEY = 'Ctrl+Alt+D'

const MODIFIERS = ['Ctrl', 'Alt', 'Shift', 'Super']
const STRONG = new Set(['Ctrl', 'Alt', 'Super'])

const NAMED_KEYS = new Set([
  'Space', 'Enter', 'Tab', 'Backspace', 'Delete', 'Insert', 'Home', 'End', 'PageUp', 'PageDown',
  'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'
])

/** KeyboardEvent.code to the key name used in accelerators, or null. */
function keyNameFromCode (code) {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit\d$/.test(code)) return code.slice(5)
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code
  if (NAMED_KEYS.has(code)) return code
  return null
}

/**
 * @param {{code: string, ctrlKey?: boolean, altKey?: boolean, shiftKey?: boolean, metaKey?: boolean}} event
 * @returns {{accelerator: string} | {error: 'MODIFIER' | 'KEY' | 'PARTIAL'}}
 *   PARTIAL: only a modifier is down so far; keep waiting for the real key.
 */
export function acceleratorFromEvent (event) {
  if (/^(Control|Alt|Shift|Meta|OS)(Left|Right)$/.test(event.code ?? '')) return { error: 'PARTIAL' }
  const key = keyNameFromCode(event.code ?? '')
  if (!key) return { error: 'KEY' }

  const mods = []
  if (event.ctrlKey) mods.push('Ctrl')
  if (event.altKey) mods.push('Alt')
  if (event.shiftKey) mods.push('Shift')
  if (event.metaKey) mods.push('Super')
  if (!mods.some((m) => STRONG.has(m))) return { error: 'MODIFIER' }
  return { accelerator: [...mods, key].join('+') }
}

/** '' means "hotkey off" and is valid. */
export function isValidAccelerator (text) {
  if (text === '') return true
  if (typeof text !== 'string') return false
  const parts = text.split('+')
  const key = parts.pop()
  if (parts.length === 0 || parts.length > MODIFIERS.length) return false
  if (new Set(parts).size !== parts.length) return false
  if (!parts.every((m) => MODIFIERS.includes(m))) return false
  if (!parts.some((m) => STRONG.has(m))) return false
  return /^[A-Z0-9]$/.test(key) || /^F([1-9]|1\d|2[0-4])$/.test(key) || NAMED_KEYS.has(key)
}

// ------------------------------------------------------- panel navigation

/**
 * Next selected id for a navigation key. `ids` are the selectable rows in
 * display order. No current selection starts at the top (or the bottom for Up).
 */
export function nextSelection (ids, current, key) {
  if (ids.length === 0) return null
  const at = ids.indexOf(current)
  switch (key) {
    case 'ArrowDown': return ids[at < 0 ? 0 : Math.min(at + 1, ids.length - 1)]
    case 'ArrowUp': return ids[at < 0 ? ids.length - 1 : Math.max(at - 1, 0)]
    case 'Home': return ids[0]
    case 'End': return ids[ids.length - 1]
    default: return current ?? ids[0]
  }
}

const SNOOZE_KEYS = { 1: '30m', 2: '2h', 3: 'tomorrow' }

/**
 * Map a key press inside the open panel to an action.
 * @param {string} key KeyboardEvent.key
 * @param {{snoozing: boolean}} mode snoozing: the 30m/2h/tomorrow chooser is showing
 * @returns {{type: string, kind?: string} | null}
 */
export function keyAction (key, { snoozing = false } = {}) {
  if (key === 'Escape') return { type: snoozing ? 'snooze-cancel' : 'close' }
  if (snoozing) {
    return SNOOZE_KEYS[key] ? { type: 'snooze', kind: SNOOZE_KEYS[key] } : null
  }
  switch (key) {
    case 'ArrowDown':
    case 'ArrowUp':
    case 'Home':
    case 'End':
      return { type: 'move', key }
    case 'Enter':
      return { type: 'open' }
    case 'd':
    case 'D':
    case ' ':
      return { type: 'done' }
    case 's':
    case 'S':
      return { type: 'snooze-menu' }
    default:
      return null
  }
}
