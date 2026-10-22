/**
 * countdown.js
 * ---------------------------------------------------------------------------
 * Urgency buckets and countdown text. Every urgency state carries a word or a
 * number as well as a color (PRD principle 3): overdue is never a bare label,
 * "soon" always shows the time left.
 *
 * Pure module. `t` is the translator from i18n.js.
 */

export const MINUTE_MS = 60 * 1000
export const HOUR_MS = 60 * MINUTE_MS
export const DAY_MS = 24 * HOUR_MS

/** Due within this window counts as "soon" (amber). */
export const SOON_MS = DAY_MS

/** overdue | soon | later. Callers leave "later" uncolored. */
export function urgencyOf (dueMs, nowMs = Date.now()) {
  const delta = dueMs - nowMs
  if (delta < 0) return 'overdue'
  if (delta <= SOON_MS) return 'soon'
  return 'later'
}

function parts (absMs) {
  const minutes = Math.floor(absMs / MINUTE_MS)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)
  return { minutes, hours, days }
}

/**
 * Compact countdown for the resting pill and list rows.
 *   upcoming: "3d 2h" | "2h 14m" | "42m" (or "42:07" with { seconds: true })
 *   overdue:  "Overdue 3h"
 */
export function compactCountdown (dueMs, nowMs, t, { seconds = false } = {}) {
  const delta = dueMs - nowMs
  const abs = Math.abs(delta)
  const { minutes, hours, days } = parts(abs)
  const m = t('unit.m')
  const h = t('unit.h')
  const d = t('unit.d')

  if (delta < 0) {
    let value
    if (minutes < 60) value = `${Math.max(minutes, 1)}${m}`
    else if (hours < 24) value = `${hours}${h}`
    else value = `${days}${d}`
    return t('overdue.ago', { value })
  }

  if (minutes < 60) {
    if (seconds) {
      const s = Math.floor(abs / 1000) % 60
      return `${String(minutes).padStart(2, '0')}:${String(s).padStart(2, '0')}`
    }
    return `${Math.max(minutes, 1)}${m}`
  }
  if (hours < 24) return `${hours}${h} ${minutes % 60}${m}`
  return `${days}${d} ${hours % 24}${h}`
}

function startOfDay (ms) {
  const date = new Date(ms)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}

/** "Today 14:00" | "Tomorrow 09:30" | "Fri 23:59" | "22 Aug" (localized). */
export function dueLabel (dueMs, nowMs, language, t) {
  const locale = language === 'id' ? 'id-ID' : 'en-GB'
  const time = new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
    .format(new Date(dueMs))
    .replace('.', ':')
  const dayDelta = Math.round((startOfDay(dueMs) - startOfDay(nowMs)) / DAY_MS)
  if (dayDelta === 0) return t('when.today', { time })
  if (dayDelta === 1) return t('when.tomorrow', { time })
  if (dayDelta > 1 && dayDelta < 7) {
    const weekday = new Intl.DateTimeFormat(locale, { weekday: 'short' }).format(new Date(dueMs))
    return `${weekday} ${time}`
  }
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short' }).format(new Date(dueMs))
}

/** True when synced data is older than a refresh interval (plus a grace minute). */
export function isDataStale (lastFetchedAtMs, nowMs, refreshMinutes = 20) {
  if (!Number.isFinite(lastFetchedAtMs)) return true
  return nowMs - lastFetchedAtMs > (Number(refreshMinutes) || 20) * MINUTE_MS + MINUTE_MS
}

/**
 * How long until the displayed countdown text can change. Used to sleep the
 * UI timer: seconds only matter in the last hour.
 */
export function nextTickMs (dueMs, nowMs, { live }) {
  if (!live) return MINUTE_MS
  const delta = dueMs - nowMs
  if (delta > 0 && delta < HOUR_MS) return 1000
  return 15 * 1000
}
