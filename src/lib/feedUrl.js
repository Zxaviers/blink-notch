/**
 * feedUrl.js
 * ---------------------------------------------------------------------------
 * Pure helpers for calendar feed URLs and refresh timing. The URL carries a
 * private access token, so nothing here logs or returns it whole except
 * `validateUrl`, which hands the cleaned value straight back to the caller.
 *
 * The network itself lives in Rust (src-tauri/src/feed.rs); the page has no
 * network access at all (CSP default-src 'self').
 *
 * Ported from the Reminder Widget v1.2 fetchCalendar.js: the 15 to 30 minute
 * refresh clamp, the offline retry ladder and the Moodle URL diagnostics.
 */

export const MINUTE_MS = 60 * 1000

/** Hard floor: never poll a campus server faster than this. */
export const MIN_REFRESH_MS = 15 * MINUTE_MS
export const DEFAULT_REFRESH_MS = 20 * MINUTE_MS
export const MAX_REFRESH_MS = 30 * MINUTE_MS

/** Retry ladder for failures where the request never reached the server. */
export const OFFLINE_RETRY_LADDER_MS = [MINUTE_MS, 2 * MINUTE_MS, 4 * MINUTE_MS, 8 * MINUTE_MS]

export class FeedUrlError extends Error {
  constructor (message, code) {
    super(message)
    this.name = 'FeedUrlError'
    this.code = code
  }
}

export function clampInterval (value) {
  if (!Number.isFinite(value)) return DEFAULT_REFRESH_MS
  return Math.min(Math.max(value, MIN_REFRESH_MS), MAX_REFRESH_MS)
}

/** Host and path only. Never include the query string (it holds the token). */
export function describeUrl (url) {
  try {
    const parsed = new URL(url)
    return `${parsed.protocol}//${parsed.host}${parsed.pathname}`
  } catch {
    return '<invalid url>'
  }
}

export function validateUrl (rawUrl) {
  if (typeof rawUrl !== 'string' || rawUrl.trim() === '') {
    throw new FeedUrlError('Calendar feed URL is not configured.', 'URL_MISSING')
  }
  let parsed
  try {
    parsed = new URL(rawUrl.trim())
  } catch {
    throw new FeedUrlError('Calendar feed URL is not a valid URL.', 'URL_INVALID')
  }
  if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') {
    throw new FeedUrlError(
      `Calendar feed URL must use http or https (got "${parsed.protocol}").`,
      'URL_PROTOCOL'
    )
  }
  return parsed.toString()
}

/**
 * Delay before the next refresh attempt.
 * @param {object} o
 * @param {number} o.refreshMs        configured interval (clamped here)
 * @param {boolean} o.ok              last attempt reached the server
 * @param {number} o.transportFailures consecutive "never reached the server" failures, including this one
 */
export function nextRefreshDelayMs ({ refreshMs, ok, transportFailures = 0 }) {
  const interval = clampInterval(refreshMs)
  if (ok || transportFailures <= 0) return interval
  if (transportFailures > OFFLINE_RETRY_LADDER_MS.length) return interval
  return OFFLINE_RETRY_LADDER_MS[transportFailures - 1]
}

/**
 * Sanity-check the shape of a Moodle export URL before spending a request.
 * Advisory only: unusual but valid deployments still get to try.
 * Returns { ok, hint } where hint is a hint code for the UI to localize.
 */
export function inspectMoodleUrl (rawUrl) {
  let parsed
  try {
    parsed = new URL(String(rawUrl).trim())
  } catch {
    return { ok: false, hint: null }
  }

  const path = parsed.pathname.toLowerCase()
  const isExport = path.includes('export_execute.php')
  const hasToken = parsed.searchParams.has('authtoken')
  if (isExport && hasToken) return { ok: true, hint: null }

  const KNOWN_PAGES = [
    [/\/my\/?$/, 'moodle_dashboard'],
    [/\/calendar\/view\.php/, 'moodle_calendar_page'],
    [/\/course\/view\.php/, 'moodle_course_page'],
    [/\/mod\//, 'moodle_activity_page'],
    [/^\/?$/, 'moodle_home']
  ]
  for (const [pattern, hint] of KNOWN_PAGES) {
    if (pattern.test(path)) return { ok: false, hint }
  }
  return { ok: false, hint: isExport ? 'moodle_missing_token' : 'moodle_not_export' }
}
