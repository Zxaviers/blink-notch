/**
 * format.js
 * ---------------------------------------------------------------------------
 * Small text helpers for the pill and the footer. Pure.
 */

export function truncate (text, max) {
  const s = String(text ?? '').trim()
  return s.length <= max ? s : `${s.slice(0, Math.max(0, max - 1)).trimEnd()}…`
}

/** A leading course code such as "CS101" or "MA 201" (letters then digits). */
const COURSE_CODE = /^([A-Za-z]{2,6})\s?-?(\d{2,4}[A-Za-z]?)(?![A-Za-z0-9])/

/** Short label for the resting pill: course code, else course name, else title. */
export function pillLabel (task, max = 14) {
  if (!task) return ''
  const course = String(task.course ?? '').trim()
  const match = COURSE_CODE.exec(course)
  if (match) return `${match[1]}${match[2]}`.toUpperCase()
  if (course) return truncate(course, max)
  return truncate(task.title, max)
}

function clock (ms, language) {
  const locale = language === 'id' ? 'id-ID' : 'en-GB'
  return new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
    .format(new Date(ms))
    .replace('.', ':')
}

/**
 * Footer status across all feeds.
 * @param {Record<string, {fetchedAtMs?: number|null, syncing?: boolean, error?: {code: string, transport: boolean}|null}>} feeds
 * @returns {{kind: 'none'|'syncing'|'ok'|'offline'|'error'|'never', text: string}}
 */
export function syncSummary (feeds, t, language) {
  const list = Object.values(feeds ?? {})
  if (list.length === 0) return { kind: 'none', text: '' }
  if (list.some((f) => f.syncing)) return { kind: 'syncing', text: t('footer.syncing') }

  const times = list.map((f) => f.fetchedAtMs).filter(Number.isFinite)
  const last = times.length ? Math.max(...times) : null

  if (list.some((f) => f.error?.transport)) {
    return {
      kind: 'offline',
      text: last === null ? t('footer.never') : t('footer.offline', { time: clock(last, language) })
    }
  }
  const failing = list.find((f) => f.error)
  if (failing) return { kind: 'error', text: t(`error.${failing.error.code}`) }
  if (last === null) return { kind: 'never', text: t('footer.never') }
  return { kind: 'ok', text: t('footer.synced', { time: clock(last, language) }) }
}
