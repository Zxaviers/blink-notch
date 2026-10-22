// Notch shell: owns the three states (rest, peek, open), the shape reported to
// Rust for click-through, hover intent, theme and the update timer. What is
// shown inside comes from the module (modules/deadlines.js).

import { api } from './api.js'
import { createCore } from './core.js'
import { clear } from './dom.js'
import { deadlinesModule, selectableIds } from './modules/deadlines.js'
import { restModeFor } from './lib/view.js'
import { HOUR_MS, urgencyOf } from './lib/countdown.js'
import { keyAction, nextSelection } from './lib/hotkey.js'

const active = deadlinesModule

const REST_SIZES = { full: { w: 232, h: 32 }, mini: { w: 84, h: 10 }, hot: { w: 160, h: 6 } }
const PEEK_SIZE = { w: 400, h: 56 }
const OPEN_W = 420
const OPEN_MAX_H = 340
const PEEK_MS = 4000
const GRACE_MS = 300

const notch = document.getElementById('notch')
const restCode = document.getElementById('rest-code')
const restCount = document.getElementById('rest-count')
const peekEl = document.getElementById('peek')
const panel = document.getElementById('panel')
const inner = document.getElementById('panel-inner')

const core = createCore({ api })
const darkQuery = window.matchMedia('(prefers-color-scheme: dark)')

let state = 'rest'
let restMode = 'full'
let openHeight = 120
let doneOpen = false
let snoozeFor = null
let hovering = false
// Keyboard mode: opened with the global hotkey, the notch holds keyboard focus
// and the arrow keys move a selection through the rows.
let keyboard = false
let selectedId = null
let intentTimer = null
let graceTimer = null
let peekTimer = null
let tickTimer = null
let lastMinute = 0

const actions = {
  markDone: (id) => core.markDone(id),
  restore: (id) => core.restore(id),
  openUrl: (url) => api.openUrl(url).catch(() => {}),
  openSettings: () => api.openSettings().catch(() => {}),
  refresh: () => core.syncAll(true),
  toggleDone: () => { doneOpen = !doneOpen; renderPanel() },
  toggleSnooze: (id) => { snoozeFor = snoozeFor === id ? null : id; renderPanel() },
  snooze: (id, kind) => { snoozeFor = null; core.snooze(id, kind) }
}

function makeCtx (nowMs = Date.now()) {
  const s = core.state
  return {
    view: core.view(nowMs),
    t: s.t,
    language: s.language,
    nowMs,
    settings: s.settings,
    feeds: s.feeds,
    hasFeeds: s.settings.feeds.some((f) => f.enabled),
    doneOpen,
    snoozed: s.snoozed,
    snoozeFor,
    keyboard,
    selectedId: keyboard ? selectedId : null,
    actions
  }
}

// ------------------------------------------------------------------- shape

function currentSize () {
  if (state === 'open') return { w: OPEN_W, h: openHeight }
  if (state === 'peek') return PEEK_SIZE
  return REST_SIZES[restMode]
}

function applyShape () {
  const { w, h } = currentSize()
  notch.style.setProperty('--w', `${w}px`)
  notch.style.setProperty('--h', `${h}px`)
  // Report the target bounds immediately: growing needs the hit area first,
  // and shrinking only happens once the cursor has already left.
  api.setShape((window.innerWidth - w) / 2, 0, w, h).catch(() => {})
}

function effectiveSurface () {
  const theme = core.state.settings.theme
  const light = theme === 'light' || (theme === 'auto' && !darkQuery.matches)
  return state === 'open' && light ? 'light' : 'dark'
}

// ------------------------------------------------------------------ render

function renderRest (ctx = makeCtx()) {
  restMode = restModeFor({
    restingStyle: ctx.settings.restingStyle,
    hero: ctx.view.hero,
    nowMs: ctx.nowMs,
    dueHorizonHours: ctx.settings.dueHorizonHours
  })
  const rest = active.rest(ctx)
  restCode.textContent = rest.label
  restCode.hidden = rest.label === ''
  restCount.textContent = rest.count
  notch.dataset.urgency = rest.urgency
  notch.dataset.rest = restMode
}

function renderPanel () {
  const scrollTop = panel.scrollTop
  clear(inner).append(active.panel(makeCtx()))
  openHeight = Math.min(inner.offsetHeight, OPEN_MAX_H)
  panel.scrollTop = scrollTop
  if (state === 'open') applyShape()
  if (keyboard) inner.querySelector('.row.kbd')?.scrollIntoView({ block: 'nearest' })
}

function renderPeek (task) {
  clear(peekEl).append(active.peek(task, makeCtx()))
}

/** Keep the selection on a row that still exists after the data changed. */
function reconcileSelection () {
  if (!keyboard) return
  const ids = selectableIds(core.view())
  if (!ids.includes(selectedId)) selectedId = ids[0] ?? null
  if (snoozeFor !== null && !ids.includes(snoozeFor)) snoozeFor = null
}

function render () {
  reconcileSelection()
  const ctx = makeCtx()
  renderRest(ctx)
  if (state === 'open') renderPanel()
  notch.dataset.surface = effectiveSurface()
  applyShape()
}

// ------------------------------------------------------------------- state

/**
 * Frame pacing of the expand animation, written to the debug log (DN_DEBUG=1
 * only; otherwise the Rust side drops it). The app runs with software
 * rendering, so this is how to check the animation still holds ~60 fps.
 */
function measureFrames (label) {
  const stamps = []
  const start = performance.now()
  const report = () => {
    const gaps = stamps.slice(1).map((t, i) => t - stamps[i]).sort((a, b) => a - b)
    if (gaps.length === 0) return
    const q = (p) => gaps[Math.min(gaps.length - 1, Math.floor(p * gaps.length))]
    api.debugLog(
      `frames ${label}: n=${gaps.length} p50=${q(0.5).toFixed(1)}ms p95=${q(0.95).toFixed(1)}ms ` +
      `max=${gaps[gaps.length - 1].toFixed(1)}ms slow(>20ms)=${gaps.filter((g) => g > 20).length}`
    ).catch(() => {})
  }
  const step = (t) => {
    stamps.push(t)
    if (t - start < 450) requestAnimationFrame(step)
    else report()
  }
  requestAnimationFrame(step)
}

function setState (next) {
  if (state === next) return
  state = next
  notch.dataset.state = next
  if (next === 'open') {
    measureFrames('open')
    renderPanel()
  }
  notch.dataset.surface = effectiveSurface()
  applyShape()
  scheduleTick()
}

function openNow () {
  clearTimeout(intentTimer)
  intentTimer = null
  clearTimeout(peekTimer)
  setState('open')
}

function onHover (inside) {
  hovering = inside
  if (inside) {
    clearTimeout(graceTimer)
    graceTimer = null
    if (state !== 'open' && intentTimer === null) {
      intentTimer = setTimeout(() => {
        intentTimer = null
        openNow()
      }, core.state.settings.hoverDelayMs)
    }
  } else {
    clearTimeout(intentTimer)
    intentTimer = null
    if (state === 'open' && graceTimer === null && !keyboard) {
      graceTimer = setTimeout(() => {
        graceTimer = null
        if (!hovering) setState('rest')
      }, GRACE_MS)
    }
  }
}

function peek (task) {
  if (state === 'open') return // already on screen
  renderPeek(task)
  setState('peek')
  clearTimeout(peekTimer)
  peekTimer = setTimeout(() => {
    if (state === 'peek') setState('rest')
  }, PEEK_MS)
}

// ----------------------------------------------------------------- keyboard

function findTask (id) {
  const { upcoming, overdue } = core.view()
  return [...upcoming, ...overdue].find((task) => task.id === id) ?? null
}

/** Hotkey: open the panel and take keyboard focus. */
async function enterKeyboard () {
  openNow()
  let focused = false
  try {
    focused = await api.enterKeyboardMode()
  } catch { /* handled below */ }

  if (!focused) {
    // Windows refused to hand over focus: behave like a plain open and close
    // again by itself, since no key presses will reach us.
    api.exitKeyboardMode().catch(() => {})
    setTimeout(() => { if (!hovering && !keyboard) setState('rest') }, 4000)
    return
  }
  keyboard = true
  selectedId = selectableIds(core.view())[0] ?? null
  renderPanel()
  panel.focus()
}

function exitKeyboard () {
  if (!keyboard) return
  keyboard = false
  selectedId = null
  snoozeFor = null
  api.exitKeyboardMode().catch(() => {})
  setState('rest')
}

function onKeyDown (event) {
  if (!keyboard) {
    if (event.key === 'Escape') setState('rest')
    return
  }
  const action = keyAction(event.key, { snoozing: snoozeFor !== null })
  if (!action) return
  event.preventDefault()

  const ids = selectableIds(core.view())
  const task = selectedId ? findTask(selectedId) : null
  switch (action.type) {
    case 'move':
      selectedId = nextSelection(ids, selectedId, action.key)
      renderPanel()
      break
    case 'open':
      if (task?.url) actions.openUrl(task.url)
      break
    case 'done':
      if (task) {
        const at = ids.indexOf(task.id)
        const id = task.id
        selectedId = ids[at + 1] ?? ids[at - 1] ?? null // move on before the row disappears
        core.markDone(id)
      }
      break
    case 'snooze-menu':
      if (task && urgencyOf(task.dueMs, Date.now()) !== 'overdue') {
        snoozeFor = task.id
        renderPanel()
      }
      break
    case 'snooze':
      if (snoozeFor !== null) {
        const id = snoozeFor
        snoozeFor = null
        core.snooze(id, action.kind)
        renderPanel()
      }
      break
    case 'snooze-cancel':
      snoozeFor = null
      renderPanel()
      break
    case 'close':
      exitKeyboard()
      break
    default:
  }
}

// -------------------------------------------------------------------- tick

/** Seconds only matter while something is visible or the next deadline is within the hour. */
function scheduleTick () {
  clearTimeout(tickTimer)
  const hero = core.view().hero
  const untilHero = hero ? hero.dueMs - Date.now() : Infinity
  let delay = 20000
  if (state !== 'rest') delay = 1000
  else if (untilHero > 0 && untilHero < HOUR_MS && restMode === 'full') delay = 1000
  tickTimer = setTimeout(onTick, delay)
}

function onTick () {
  const ctx = makeCtx()
  renderRest(ctx)
  if (state === 'open') active.tickPanel(inner, ctx)
  else if (state === 'peek') active.tickPanel(peekEl, ctx)
  applyShape()

  const minute = Math.floor(ctx.nowMs / 60000)
  if (minute !== lastMinute) {
    lastMinute = minute
    core.tick(ctx.nowMs).catch(() => {})
  }
  scheduleTick()
}

// ------------------------------------------------------------------ wiring

notch.addEventListener('click', () => {
  if (state !== 'open') openNow()
})
window.addEventListener('keydown', onKeyDown)
// Clicking another app (or Alt+Tab) while the notch holds keyboard focus ends keyboard mode.
window.addEventListener('blur', () => exitKeyboard())
window.addEventListener('resize', applyShape)
darkQuery.addEventListener('change', render)

core.subscribe(() => {
  render()
  scheduleTick()
})
core.onPeek(peek)

api.on('notch-hover', onHover)
api.on('hotkey', () => (keyboard ? exitKeyboard() : enterKeyboard()))
api.on('refresh-now', () => core.syncAll(true))
api.on('settings-changed', () => core.reloadSettings())
api.on('data-changed', () => core.reloadLocal())

applyShape()
api.debugLog('page script running').catch(() => {})
core.start()
  .then(() => api.debugLog('core ready (settings loaded, first feed sync done)'))
  .catch(() => {})
scheduleTick()

// Debug-only: scripts/stress-cost.ps1 sets DN_STRESS to measure sustained
// animation cost. stress.js is never loaded otherwise.
api.debugFlags().then((flags) => {
  if (flags?.stress) import('./stress.js').then((m) => m.start(flags.stress, { open: openNow, inner })).catch(() => {})
}).catch(() => {})
