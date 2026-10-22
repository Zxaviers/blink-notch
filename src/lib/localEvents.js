'use strict'

/**
 * localEvents.js
 * ---------------------------------------------------------------------------
 * Pure helpers for manual ("local") events: deadlines the user types in
 * themselves (meetings, personal deadlines) that no calendar feed knows about.
 *
 * Events persist as plain objects in the local-events store file and are
 * merged into the feed task list as task-shaped records with `source: 'local'`,
 * so notifications, mark-done, countdowns and the Android widget sync all reuse
 * the existing pipeline unchanged.
 *
 * Pure module: no DOM, no storage, no network. Safe to unit test and to run in
 * both the renderer and Node's test runner.
 */

import { DEFAULT_KEEP_OVERDUE_MS, DEFAULT_HORIZON_MS } from './parseTasks.js'

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/
const TIME_RE = /^\d{2}:\d{2}$/

/**
 * Validate raw form input and normalise it into a persistable event.
 * Missing time means an all-day event, due at 23:59 local (same convention as
 * all-day feed events in parseTasks).
 *
 * @returns {{ok: true, event: object} | {ok: false, error: string}}
 */
export function normalizeLocalEvent ({ title, date, time } = {}, nowMs = Date.now()) {
  const cleanTitle = String(title ?? '').trim()
  if (!cleanTitle) return { ok: false, error: 'TITLE_REQUIRED' }

  const rawDate = String(date ?? '').trim()
  if (!DATE_RE.test(rawDate)) return { ok: false, error: 'DATE_REQUIRED' }

  const rawTime = String(time ?? '').trim()
  if (rawTime !== '' && !TIME_RE.test(rawTime)) return { ok: false, error: 'TIME_INVALID' }

  const hhmm = rawTime === '' ? '23:59' : rawTime
  const dueMs = new Date(`${rawDate}T${hhmm}:00`).getTime()
  if (!Number.isFinite(dueMs)) return { ok: false, error: 'DATE_INVALID' }

  // Same visibility window as feed tasks: not older than the keep-overdue
  // window, not further ahead than the horizon.
  if (dueMs < nowMs - DEFAULT_KEEP_OVERDUE_MS) {
    return { ok: false, error: 'DATE_PAST' }
  }
  if (dueMs > nowMs + DEFAULT_HORIZON_MS) {
    return { ok: false, error: 'DATE_FAR' }
  }

  return {
    ok: true,
    event: {
      id: `local-${nowMs.toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
      title: cleanTitle,
      dueMs,
      allDay: rawTime === ''
    }
  }
}

/** Shape a persisted event like a parsed feed task. */
export function localTaskFrom (ev) {
  return {
    id: ev.id,
    uid: ev.id,
    title: ev.title,
    course: '',
    phase: 'due',
    due: new Date(ev.dueMs).toISOString(),
    dueMs: ev.dueMs,
    allDay: ev.allDay === true,
    description: '',
    url: null,
    source: 'local'
  }
}

/**
 * Feed tasks + live local events, nearest deadline first. Events older than
 * the keep-overdue window drop out silently (they can never be acted on).
 * The input arrays are not mutated.
 */
export function withLocalTasks (feedTasks, events, nowMs = Date.now(), keepOverdueMs = DEFAULT_KEEP_OVERDUE_MS) {
  const feed = Array.isArray(feedTasks) ? feedTasks : []
  const minMs = nowMs - keepOverdueMs
  const feedIds = new Set(feed.map((t) => t.id))
  const locals = (Array.isArray(events) ? events : [])
    .filter((ev) => ev && Number.isFinite(ev.dueMs) && ev.dueMs >= minMs && !feedIds.has(ev.id))
    .map(localTaskFrom)
  return [...feed, ...locals].sort(
    (a, b) => a.dueMs - b.dueMs || a.title.localeCompare(b.title)
  )
}
