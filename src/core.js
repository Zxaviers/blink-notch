// Notch core: loads settings and data, keeps feeds in sync, plans
// notifications. No DOM here; the shell subscribes and renders.

import { parseTasks, DAY_MS } from './lib/parseTasks.js'
import { normalizeSettings } from './lib/settings.js'
import { createT, resolveLanguage } from './lib/i18n.js'
import { buildView } from './lib/view.js'
import { nextRefreshDelayMs, MINUTE_MS } from './lib/feedUrl.js'
import {
  planNotifications, recordFired, pruneFired, groupNotifications,
  snoozeUntil, expireSnoozes
} from './lib/planner.js'
import { compactCountdown } from './lib/countdown.js'
import { createDoneStore } from './lib/doneStore.js'

/** Parse generously; the view applies the user's keep-overdue window. */
const PARSE_KEEP_MS = 30 * DAY_MS
const PARSE_HORIZON_MS = 365 * DAY_MS

export function createCore ({ api }) {
  const listeners = new Set()
  const peekListeners = new Set()
  const timers = new Map()

  const state = {
    ready: false,
    settings: normalizeSettings(null),
    language: 'en',
    t: createT('en'),
    /** feed id -> { tasks, fetchedAtMs, fromCache, error, syncing, failures } */
    feeds: {},
    localEvents: [],
    done: {},
    fired: {},
    /** taskId -> epoch ms until which reminders stay quiet */
    snoozed: {}
  }

  const emitChange = () => listeners.forEach((fn) => fn(state))

  function applyLanguage () {
    state.language = resolveLanguage(state.settings.language, navigator.language)
    state.t = createT(state.language)
    document.documentElement.lang = state.language
  }

  async function pushTrayLabels () {
    const t = state.t
    await api.setTrayLabels({
      pause: t('tray.pause'),
      refresh: t('tray.refresh'),
      settings: t('tray.settings'),
      quit: t('tray.quit')
    }).catch(() => {})
  }

  function enabledFeeds () {
    return state.settings.feeds.filter((f) => f.enabled)
  }

  function view (nowMs = Date.now()) {
    const feedResults = enabledFeeds()
      .map((feed) => ({ feed, tasks: state.feeds[feed.id]?.tasks ?? [] }))
    return buildView({
      feedResults,
      localEvents: state.localEvents,
      done: state.done,
      settings: state.settings,
      nowMs
    })
  }

  // ----------------------------------------------------------------- feeds

  function clearTimer (id) {
    clearTimeout(timers.get(id))
    timers.delete(id)
  }

  function schedule (id, delayMs) {
    clearTimer(id)
    timers.set(id, setTimeout(() => syncFeed(id, false), delayMs))
  }

  async function syncFeed (id, force) {
    const feed = state.settings.feeds.find((f) => f.id === id && f.enabled)
    if (!feed) return
    const entry = state.feeds[id] ?? { tasks: [], fetchedAtMs: null, fromCache: false, error: null, failures: 0 }
    state.feeds[id] = { ...entry, syncing: true }
    emitChange()

    let result
    try {
      result = await api.feedFetch(id, force)
    } catch {
      result = { ics: null, fromCache: false, fetchedAtMs: null, error: { code: 'NETWORK', transport: true } }
    }

    let tasks = entry.tasks
    let error = result.error ?? null
    if (typeof result.ics === 'string') {
      try {
        tasks = parseTasks(result.ics, {
          now: new Date(),
          keepOverdueMs: PARSE_KEEP_MS,
          horizonMs: PARSE_HORIZON_MS
        })
      } catch {
        error = { code: 'NOT_CALENDAR', transport: false }
      }
    }

    const transportFailed = Boolean(error?.transport)
    const failures = transportFailed ? entry.failures + 1 : 0
    state.feeds[id] = {
      tasks,
      fetchedAtMs: result.fetchedAtMs ?? entry.fetchedAtMs,
      fromCache: result.fromCache,
      error,
      syncing: false,
      failures
    }
    schedule(id, nextRefreshDelayMs({
      refreshMs: state.settings.refreshMinutes * MINUTE_MS,
      ok: !error,
      transportFailures: failures
    }))
    emitChange()
    await planAndNotify()
  }

  async function syncAll (force = false) {
    await Promise.all(enabledFeeds().map((feed) => syncFeed(feed.id, force)))
  }

  /** Reconcile after settings changed: drop removed feeds, sync new or re-enabled ones. */
  async function reconcileFeeds () {
    const wanted = new Set(enabledFeeds().map((f) => f.id))
    for (const id of Object.keys(state.feeds)) {
      if (!wanted.has(id)) {
        clearTimer(id)
        delete state.feeds[id]
      }
    }
    emitChange()
    await Promise.all([...wanted].map((id) => (state.feeds[id] ? null : syncFeed(id, false))))
  }

  // -------------------------------------------------------- notifications

  // Serialized: several feeds can finish at once, and each pass reads then
  // writes the fired map, so overlapping passes would announce twice.
  let planChain = Promise.resolve()
  function planAndNotify (nowMs = Date.now()) {
    planChain = planChain.then(() => runPlan(nowMs)).catch(() => {})
    return planChain
  }

  async function runPlan (nowMs) {
    const { upcoming } = view(nowMs)
    const plans = planNotifications(upcoming, state.settings.thresholdsHours, nowMs, state.fired, state.snoozed)
    if (plans.length === 0) return

    const byId = new Map(upcoming.map((task) => [task.id, task]))
    const { individual, extra } = groupNotifications(plans)
    const t = state.t

    for (const plan of individual) {
      const task = byId.get(plan.taskId)
      if (!task) continue
      if (state.settings.toast) {
        api.notify(
          t('toast.title', { title: task.title }),
          t('toast.body', { course: task.course ?? '', left: compactCountdown(task.dueMs, nowMs, t) })
        ).catch(() => {})
      }
      peekListeners.forEach((fn) => fn(task))
    }
    if (extra > 0 && state.settings.toast) {
      api.notify(t('toast.summary.title', { n: plans.length }), t('toast.summary.body')).catch(() => {})
    }

    state.fired = recordFired(state.fired, plans)
    await api.storeSet('fired', state.fired).catch(() => {})
  }

  async function tick (nowMs = Date.now()) {
    const expired = expireSnoozes(state.snoozed, state.fired, nowMs)
    if (expired.changed) {
      state.snoozed = expired.snoozed
      state.fired = expired.fired
      await Promise.all([api.storeSet('snoozed', state.snoozed), api.storeSet('fired', state.fired)]).catch(() => {})
      emitChange()
    }
    const { upcoming, overdue, doneList } = view(nowMs)
    const kept = [...upcoming, ...overdue, ...doneList]
    const pruned = pruneFired(state.fired, kept, state.settings.keepOverdueDays * DAY_MS, nowMs)
    if (Object.keys(pruned).length !== Object.keys(state.fired).length) {
      state.fired = pruned
      await api.storeSet('fired', state.fired).catch(() => {})
    }
    await planAndNotify(nowMs)
  }

  // ------------------------------------------------------------- user actions

  async function persistDone (store) {
    state.done = store.toJSON()
    emitChange()
    await api.storeSet('done', state.done).catch(() => {})
  }

  async function markDone (taskId) {
    const store = createDoneStore(state.done)
    store.mark(taskId)
    await persistDone(store)
  }

  async function restore (taskId) {
    const store = createDoneStore(state.done)
    store.unmark(taskId)
    await persistDone(store)
  }

  /** Silence this task's reminders for a while; it is announced again afterwards. */
  async function snooze (taskId, kind) {
    const until = snoozeUntil(kind, Date.now())
    if (until === null) return
    state.snoozed = { ...state.snoozed, [taskId]: until }
    emitChange()
    await api.storeSet('snoozed', state.snoozed).catch(() => {})
  }

  // --------------------------------------------------------------- loading

  async function loadLocal () {
    const [events, done, fired, snoozed] = await Promise.all([
      api.storeGet('local_events').catch(() => null),
      api.storeGet('done').catch(() => null),
      api.storeGet('fired').catch(() => null),
      api.storeGet('snoozed').catch(() => null)
    ])
    state.localEvents = Array.isArray(events) ? events : []
    state.done = done && typeof done === 'object' ? done : {}
    state.fired = fired && typeof fired === 'object' ? fired : {}
    state.snoozed = snoozed && typeof snoozed === 'object' ? snoozed : {}
  }

  async function loadSettings () {
    const raw = await api.storeGet('settings').catch(() => null)
    state.settings = normalizeSettings(raw)
    applyLanguage()
    await api.setPlacement(state.settings.monitor, state.settings.offsetX).catch(() => {})
    await pushTrayLabels()
  }

  async function start () {
    await loadLocal()
    await loadSettings()
    state.ready = true
    emitChange()
    await syncAll(false)
    await tick()
    if (!state.settings.onboarded && state.settings.feeds.length === 0) {
      api.openSettings().catch(() => {})
    }
  }

  async function reloadSettings () {
    await loadSettings()
    await reconcileFeeds()
    emitChange()
    await tick()
  }

  async function reloadLocal () {
    await loadLocal()
    emitChange()
    await tick()
  }

  return {
    state,
    view,
    start,
    tick,
    syncAll,
    markDone,
    restore,
    snooze,
    reloadSettings,
    reloadLocal,
    subscribe (fn) { listeners.add(fn); return () => listeners.delete(fn) },
    onPeek (fn) { peekListeners.add(fn); return () => peekListeners.delete(fn) }
  }
}
