/**
 * settings.js
 * ---------------------------------------------------------------------------
 * Defaults and validation for settings.json. Whatever is read from disk or a
 * form goes through `normalizeSettings`, so the rest of the app only ever sees
 * clean values. Feed URLs are never part of settings (they live in the Windows
 * Credential Manager); only non-secret metadata rows do.
 */

import { normalizeThresholdsHours } from './notifyConfig.js'
import { clampInterval, MINUTE_MS, DEFAULT_REFRESH_MS } from './feedUrl.js'
import { FEED_KINDS, isValidFeedId } from './feeds.js'
import { DEFAULT_HOTKEY, isValidAccelerator } from './hotkey.js'

export const THEMES = ['dark', 'light', 'auto']
export const LANGUAGE_SETTINGS = ['auto', 'en', 'id']
export const RESTING_STYLES = ['always', 'when-due', 'hover']

export const DEFAULT_SETTINGS = Object.freeze({
  version: 1,
  language: 'auto',
  theme: 'dark',
  restingStyle: 'always',
  dueHorizonHours: 72,
  hoverDelayMs: 150,
  refreshMinutes: DEFAULT_REFRESH_MS / MINUTE_MS,
  thresholdsHours: [24, 6, 1],
  toast: true,
  hotkey: DEFAULT_HOTKEY,
  keepOverdueDays: 3,
  monitor: 0,
  offsetX: 0,
  onboarded: false,
  feeds: []
})

function pick (value, allowed, fallback) {
  return allowed.includes(value) ? value : fallback
}

function int (value, min, max, fallback) {
  const n = Math.round(Number(value))
  if (!Number.isFinite(n)) return fallback
  return Math.min(Math.max(n, min), max)
}

function normalizeFeedRows (rows) {
  if (!Array.isArray(rows)) return []
  const seen = new Set()
  const out = []
  for (const row of rows) {
    if (!row || !isValidFeedId(row.id) || !FEED_KINDS.includes(row.kind) || seen.has(row.id)) continue
    seen.add(row.id)
    out.push({
      id: row.id,
      kind: row.kind,
      label: String(row.label ?? '').trim().slice(0, 60) || row.id,
      enabled: row.enabled !== false
    })
  }
  return out
}

export function normalizeSettings (raw) {
  const s = raw && typeof raw === 'object' ? raw : {}
  const d = DEFAULT_SETTINGS
  return {
    version: 1,
    language: pick(s.language, LANGUAGE_SETTINGS, d.language),
    theme: pick(s.theme, THEMES, d.theme),
    restingStyle: pick(s.restingStyle, RESTING_STYLES, d.restingStyle),
    dueHorizonHours: int(s.dueHorizonHours, 1, 336, d.dueHorizonHours),
    hoverDelayMs: int(s.hoverDelayMs, 0, 1000, d.hoverDelayMs),
    refreshMinutes: clampInterval(Number(s.refreshMinutes) * MINUTE_MS) / MINUTE_MS,
    thresholdsHours: normalizeThresholdsHours(s.thresholdsHours),
    toast: s.toast !== false,
    // '' turns the hotkey off; anything unusable falls back to the default.
    hotkey: isValidAccelerator(s.hotkey) ? s.hotkey : d.hotkey,
    keepOverdueDays: int(s.keepOverdueDays, 0, 30, d.keepOverdueDays),
    monitor: int(s.monitor, 0, 15, d.monitor),
    offsetX: int(s.offsetX, -2000, 2000, d.offsetX),
    onboarded: s.onboarded === true,
    feeds: normalizeFeedRows(s.feeds)
  }
}
