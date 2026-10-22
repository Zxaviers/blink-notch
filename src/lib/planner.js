/**
 * planner.js
 * ---------------------------------------------------------------------------
 * Notification planner. A pure function says which deadline thresholds are due
 * right now, given the tasks, the thresholds and a record of what already
 * fired. Each threshold fires once per task.
 *
 * Catch-up rule: if several thresholds were crossed while the app was closed
 * (say 24h and 6h), only the tightest one is announced and the rest are marked
 * fired, so a restart never replays old reminders.
 */

import { HOUR_MS } from './countdown.js'

/**
 * @param {Array<{id: string, title: string, course?: string|null, dueMs: number}>} tasks not-done tasks
 * @param {number[]} thresholdsHours e.g. [24, 6, 1]
 * @param {number} nowMs
 * @param {Record<string, number[]>} fired taskId -> thresholds already announced
 * @param {Record<string, number>} snoozed taskId -> epoch ms until which it stays quiet
 * @returns {Array<{taskId: string, thresholdHours: number, crossed: number[], remainingMs: number}>}
 */
export function planNotifications (tasks, thresholdsHours, nowMs, fired = {}, snoozed = {}) {
  if (!Array.isArray(tasks) || !Array.isArray(thresholdsHours)) return []
  const thresholds = [...new Set(thresholdsHours.filter((h) => Number.isFinite(h) && h > 0))]
    .sort((a, b) => b - a)

  const plans = []
  for (const task of tasks) {
    if (!task || typeof task.id !== 'string' || !Number.isFinite(task.dueMs)) continue
    const remainingMs = task.dueMs - nowMs
    if (remainingMs <= 0) continue // overdue items get no new alarms
    if (isSnoozed(snoozed, task.id, nowMs)) continue

    const already = Array.isArray(fired[task.id]) ? fired[task.id] : []
    const crossed = thresholds.filter((h) => remainingMs <= h * HOUR_MS && !already.includes(h))
    if (crossed.length === 0) continue

    plans.push({
      taskId: task.id,
      thresholdHours: Math.min(...crossed),
      crossed,
      remainingMs
    })
  }
  // Most urgent first.
  return plans.sort((a, b) => a.remainingMs - b.remainingMs)
}

/** Fired map with every crossed threshold of every plan recorded. Input is not mutated. */
export function recordFired (fired, plans) {
  const next = {}
  for (const [id, list] of Object.entries(fired ?? {})) next[id] = [...list]
  for (const plan of plans) {
    const set = new Set(next[plan.taskId] ?? [])
    for (const h of plan.crossed) set.add(h)
    next[plan.taskId] = [...set].sort((a, b) => b - a)
  }
  return next
}

/**
 * Drop entries for tasks that have left the list and whose deadline is outside
 * the keep-overdue window: they can never fire again.
 */
export function pruneFired (fired, tasks, keepOverdueMs, nowMs) {
  const live = new Set(tasks.map((task) => task.id))
  const minDue = nowMs - keepOverdueMs
  const next = {}
  for (const [id, list] of Object.entries(fired ?? {})) {
    if (live.has(id)) {
      next[id] = list
      continue
    }
    const dueMs = Number(id.split('::')[1])
    if (Number.isFinite(dueMs) && dueMs >= minDue) next[id] = list
  }
  return next
}

/**
 * Split plans into individual toasts and a summary count, so the first sync of
 * a big feed does not produce a wall of notifications.
 */
export function groupNotifications (plans, maxIndividual = 3) {
  return {
    individual: plans.slice(0, maxIndividual),
    extra: Math.max(0, plans.length - maxIndividual)
  }
}

// ----------------------------------------------------------------- snooze

export const SNOOZE_KINDS = ['30m', '2h', 'tomorrow']

/** "Tomorrow" means 08:00 local the next day. */
const TOMORROW_HOUR = 8

export function snoozeUntil (kind, nowMs) {
  if (kind === '30m') return nowMs + 30 * 60 * 1000
  if (kind === '2h') return nowMs + 2 * HOUR_MS
  if (kind === 'tomorrow') {
    const d = new Date(nowMs)
    d.setDate(d.getDate() + 1)
    d.setHours(TOMORROW_HOUR, 0, 0, 0)
    return d.getTime()
  }
  return null
}

export function isSnoozed (snoozed, taskId, nowMs) {
  const until = snoozed?.[taskId]
  return Number.isFinite(until) && until > nowMs
}

/**
 * Drop snoozes that have run out (and snoozes for tasks that are gone). A
 * task coming back from snooze is announced again, so its fired thresholds
 * are cleared. Inputs are not mutated.
 * @returns {{snoozed: Record<string, number>, fired: Record<string, number[]>, changed: boolean}}
 */
export function expireSnoozes (snoozed, fired, nowMs) {
  const nextSnoozed = {}
  const nextFired = { ...(fired ?? {}) }
  let changed = false
  for (const [id, until] of Object.entries(snoozed ?? {})) {
    if (Number.isFinite(until) && until > nowMs) {
      nextSnoozed[id] = until
    } else {
      changed = true
      delete nextFired[id]
    }
  }
  return { snoozed: nextSnoozed, fired: nextFired, changed }
}