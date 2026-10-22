/**
 * view.js
 * ---------------------------------------------------------------------------
 * Turns raw state (feed payloads, personal events, done map, settings, clock)
 * into what the notch shows. Pure: the UI only renders what comes out.
 */

import { mergeFeedTasks } from './multiFetch.js'
import { withLocalTasks } from './localEvents.js'
import { createDoneStore } from './doneStore.js'
import { DAY_MS } from './countdown.js'
import { HOUR_MS } from './countdown.js'

/**
 * @param {object} input
 * @param {Array<{feed: object, tasks: Array}>} input.feedResults parsed tasks per enabled feed
 * @param {Array} input.localEvents
 * @param {Record<string, string>} input.done taskId -> ISO timestamp
 * @param {{keepOverdueDays: number}} input.settings
 * @param {number} input.nowMs
 */
export function buildView ({ feedResults, localEvents, done, settings, nowMs }) {
  const keepMs = settings.keepOverdueDays * DAY_MS
  const merged = withLocalTasks(mergeFeedTasks(feedResults), localEvents, nowMs, keepMs)
  const all = merged.filter((task) => task.dueMs >= nowMs - keepMs)

  const doneStore = createDoneStore(done)
  const visible = doneStore.visible(all)
  const doneList = doneStore.doneList(all)

  const upcoming = visible.filter((task) => task.dueMs >= nowMs)
  const overdue = visible
    .filter((task) => task.dueMs < nowMs)
    .sort((a, b) => b.dueMs - a.dueMs) // most recently missed first

  return {
    upcoming,
    overdue,
    doneList,
    hero: upcoming[0] ?? overdue[0] ?? null
  }
}

/**
 * Resting pill form for the current data and the "when resting" setting.
 *   full: course code plus countdown
 *   mini: small plain pill
 *   hot:  invisible hover strip along the top edge
 */
export function restModeFor ({ restingStyle, hero, nowMs, dueHorizonHours }) {
  if (restingStyle === 'hover') return 'hot'
  if (restingStyle === 'always') return 'full'
  if (hero && (hero.dueMs < nowMs || hero.dueMs - nowMs <= dueHorizonHours * HOUR_MS)) return 'full'
  return 'mini'
}
