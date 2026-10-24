// Settings window, in two separate scopes (the notch opens the one it was asked from):
//   deadlines (the gear on the Deadlines page): calendar feeds, notifications, my events, the pill;
//   app (the three dots in the header): display, pages, weather, music, general.
// Plus the first-run wizard. Changes are written through the Rust store and announced to the notch with
// `settings-changed` / `data-changed` / `host-changed` (the pages and their options live in host.json).

import { api } from './api.js'
import { h, clear } from './dom.js'
import { normalizeSettings } from './lib/settings.js'
import { createT, resolveLanguage } from './lib/i18n.js'
import { normalizeFeed, feedMetaFrom, FEED_KINDS } from './lib/feeds.js'
import { validateUrl, inspectMoodleUrl } from './lib/feedUrl.js'
import { parseTasks, DAY_MS } from './lib/parseTasks.js'
import { normalizeLocalEvent } from './lib/localEvents.js'
import { dueLabel } from './lib/countdown.js'
import { acceleratorFromEvent, DEFAULT_HOTKEY } from './lib/hotkey.js'

const SCOPES = {
  deadlines: ['feeds', 'notifications', 'events', 'pill'],
  app: ['display', 'pages', 'weather', 'music', 'general']
}
/** The pages Blink knows, in the order they turn by default. */
const PAGE_IDS = ['clock', 'deadlines', 'controls', 'music', 'projects', 'system']
const scopeOf = (name) => (SCOPES.deadlines.includes(name) ? 'deadlines' : 'app')

const nav = document.getElementById('nav')
const main = document.getElementById('main')
const wizard = document.getElementById('wizard')

let settings = normalizeSettings(null)
let localEvents = []
let language = 'en'
let t = createT('en')
let scope = 'deadlines'
let section = 'feeds'
let host = { modules: [] }
let wizardStep = 1
let wizardKind = null

const errorText = (code) => t(`error.${typeof code === 'string' ? code : 'NETWORK'}`)

function applyLanguage () {
  language = resolveLanguage(settings.language, navigator.language)
  t = createT(language)
  document.documentElement.lang = language
  document.title = t(`settings.title.${scope}`)
}

// ------------------------------------------------------- host.json (pages)

/** Every page in order: the saved ones first, then any the file does not mention yet. */
function hostModules () {
  const saved = Array.isArray(host.modules) ? host.modules.filter((m) => m && PAGE_IDS.includes(m.id)) : []
  const seen = new Set()
  const list = []
  for (const m of saved) {
    if (seen.has(m.id)) continue
    seen.add(m.id)
    list.push({ id: m.id, enabled: m.enabled !== false, options: m.options && typeof m.options === 'object' ? m.options : {} })
  }
  for (const id of PAGE_IDS) if (!seen.has(id)) list.push({ id, enabled: true, options: {} })
  return list
}

async function saveHost (modules) {
  host = { ...host, modules }
  try {
    await api.storeSet('host', host)
  } catch {
    return false
  }
  api.emit('host-changed')
  return true
}

const optionsOf = (id) => hostModules().find((m) => m.id === id)?.options ?? {}

/** Change some options of one page (an undefined value removes the option). */
function setOptions (id, patch) {
  const modules = hostModules().map((m) => {
    if (m.id !== id) return m
    const options = { ...m.options, ...patch }
    for (const key of Object.keys(options)) if (options[key] === undefined) delete options[key]
    return { ...m, options }
  })
  return saveHost(modules)
}

async function save (patch) {
  const previousLanguage = settings.language
  settings = normalizeSettings({ ...settings, ...patch })
  try {
    await api.storeSet('settings', settings)
  } catch {
    return false
  }
  api.emit('settings-changed')
  if (settings.language !== previousLanguage) {
    applyLanguage()
    renderAll()
  }
  return true
}

async function saveLocalEvents (events) {
  await api.storeSet('local_events', events)
  localEvents = events
  api.emit('data-changed')
}

// ------------------------------------------------------------------ helpers

function field (label, control, hint) {
  const id = `f-${Math.random().toString(36).slice(2, 8)}`
  control.id = id
  return h('div', { class: 'field' },
    h('label', { for: id }, label),
    control,
    hint && h('span', { class: 'hint' }, hint))
}

function numberInput (value, { min, max, step = 1 }, onChange) {
  return h('input', {
    type: 'number', min, max, step, value,
    onchange: (e) => onChange(e.target.value)
  })
}

function select (options, value, onChange) {
  return h('select', { onchange: (e) => onChange(e.target.value) },
    options.map(([v, label]) => h('option', { value: v, selected: String(v) === String(value) }, label)))
}

function status () {
  const el = h('p', { class: 'status', role: 'status' })
  el.set = (kind, text) => { el.className = `status ${kind}`; el.textContent = text }
  return el
}

// ------------------------------------------------------------ add-feed form

/** Test-then-save form, shared by the feeds section and the wizard. */
function addFeedForm ({ kind: initialKind, onSaved }) {
  let kind = initialKind
  let testedUrl = ''
  const out = status()
  const help = h('p', {}, t(`feeds.help.${kind}`))
  const nameInput = h('input', { type: 'text', class: 'wide', maxlength: 60, autocomplete: 'off' })
  const urlInput = h('input', {
    type: 'text', class: 'wide', autocomplete: 'off', spellcheck: 'false',
    oninput: () => { testedUrl = ''; saveBtn.disabled = true }
  })
  const testBtn = h('button', { class: 'btn', type: 'button', onclick: runTest }, t('feeds.test'))
  const saveBtn = h('button', { class: 'btn primary', type: 'button', disabled: true, onclick: runSave }, t('feeds.save'))

  const kindSelect = select(
    FEED_KINDS.map((k) => [k, t(`feeds.kind.${k}`)]),
    kind,
    (value) => { kind = value; help.textContent = t(`feeds.help.${kind}`); testedUrl = ''; saveBtn.disabled = true }
  )

  async function runTest () {
    let url
    try {
      url = validateUrl(urlInput.value)
    } catch (e) {
      out.set('error', errorText(e.code))
      return
    }
    const hint = kind === 'moodle' ? inspectMoodleUrl(url) : { ok: true, hint: null }
    out.set('', t('feeds.testing'))
    testBtn.disabled = true
    try {
      const res = await api.feedTest(url)
      if (res.error) {
        out.set('error', `${errorText(res.error.code)}${hint.hint ? ` ${t(`hint.${hint.hint}`)}` : ''}`)
        return
      }
      const tasks = parseTasks(res.ics, { now: new Date(), keepOverdueMs: 3 * DAY_MS, horizonMs: 365 * DAY_MS })
      const now = Date.now()
      const next = tasks.find((task) => task.dueMs >= now)
      out.set('ok', t('feeds.test.ok', {
        n: tasks.length,
        next: next ? `${next.title} (${dueLabel(next.dueMs, now, language, t)})` : ''
      }))
      testedUrl = url
      saveBtn.disabled = false
    } catch (e) {
      out.set('error', errorText(e))
    } finally {
      testBtn.disabled = false
    }
  }

  async function runSave () {
    if (!testedUrl) {
      out.set('error', t('feeds.test.first'))
      return
    }
    const id = `${kind}-${Date.now().toString(36)}`
    const parsed = normalizeFeed({ id, kind, label: nameInput.value || t(`feeds.kind.${kind}`), url: testedUrl })
    if (!parsed.ok) {
      out.set('error', errorText(parsed.error))
      return
    }
    saveBtn.disabled = true
    try {
      await api.feedSave(id, parsed.feed.url)
    } catch (e) {
      out.set('error', errorText(e))
      return
    }
    // The URL is a secret: drop it from the page as soon as it is stored.
    urlInput.value = ''
    testedUrl = ''
    const ok = await save({ feeds: [...settings.feeds, feedMetaFrom(parsed.feed)] })
    if (!ok) {
      out.set('error', t('error.STORE'))
      return
    }
    out.set('ok', t('feeds.saved'))
    nameInput.value = ''
    onSaved?.()
  }

  return h('div', {},
    field(t('feeds.kind'), kindSelect),
    help,
    field(t('feeds.label'), nameInput),
    field(t('feeds.url'), urlInput),
    h('div', { class: 'inline' }, testBtn, saveBtn),
    out)
}

// ----------------------------------------------------------------- sections

function feedsSection () {
  const list = h('ul', { class: 'list' })
  if (settings.feeds.length === 0) list.append(h('li', { class: 'empty' }, t('feeds.empty')))
  for (const feed of settings.feeds) {
    list.append(h('li', {},
      h('div', { class: 'grow' },
        h('div', { class: 'name' }, feed.label),
        h('div', { class: 'meta' }, `${t(`feeds.kind.${feed.kind}`)} · ${t('feeds.url.saved')}`)),
      h('label', { class: 'inline' },
        h('input', {
          type: 'checkbox', checked: feed.enabled,
          onchange: (e) => save({ feeds: settings.feeds.map((f) => (f.id === feed.id ? { ...f, enabled: e.target.checked } : f)) })
        }),
        t('feeds.enabled')),
      h('button', {
        class: 'btn danger', type: 'button',
        onclick: async () => {
          if (!window.confirm(t('feeds.confirmRemove'))) return
          await api.feedRemove(feed.id).catch(() => {})
          await save({ feeds: settings.feeds.filter((f) => f.id !== feed.id) })
          renderAll()
        }
      }, t('feeds.remove'))))
  }

  return h('section', {},
    h('h1', {}, t('feeds.title')),
    h('p', {}, t('feeds.intro')),
    list,
    h('h2', {}, t('feeds.add')),
    h('div', { class: 'card' }, addFeedForm({ kind: 'moodle', onSaved: renderAll })))
}

function notificationsSection () {
  const inputs = [0, 1, 2].map((i) => h('input', {
    type: 'number', min: 0.5, max: 336, step: 0.5,
    value: settings.thresholdsHours[i] ?? '',
    'aria-label': t('notifications.threshold', { n: i + 1 }),
    onchange: () => {
      const hours = inputs.map((el) => Number(el.value)).filter((n) => Number.isFinite(n) && n > 0)
      save({ thresholdsHours: hours }).then(renderAll)
    }
  }))

  return h('section', {},
    h('h1', {}, t('notifications.title')),
    h('div', { class: 'field thresholds' },
      h('span', { class: 'label' }, t('notifications.thresholds')),
      h('div', { class: 'inline' }, inputs)),
    h('label', { class: 'check' },
      h('input', { type: 'checkbox', checked: settings.toast, onchange: (e) => save({ toast: e.target.checked }) }),
      t('notifications.toast')),
    field(t('notifications.refresh'), numberInput(settings.refreshMinutes, { min: 15, max: 30 }, (v) => save({ refreshMinutes: v }).then(renderAll))),
    field(t('notifications.keepOverdue'), numberInput(settings.keepOverdueDays, { min: 0, max: 30 }, (v) => save({ keepOverdueDays: v }).then(renderAll))))
}

/** Press-to-record field for the global hotkey. Tries to register before saving. */
function hotkeyControl () {
  const out = status()
  const label = () => settings.hotkey || t('hotkey.off')
  const shown = h('input', { type: 'text', readonly: true, class: 'hotkey-input', value: label(), 'aria-label': t('hotkey.label') })
  let capturing = false

  const stop = () => {
    capturing = false
    shown.value = label()
  }
  const apply = async (accelerator) => {
    try {
      await api.setHotkey(accelerator)
    } catch (e) {
      out.set('error', errorText(e))
      stop()
      return
    }
    const saved = await save({ hotkey: accelerator })
    out.set(saved ? 'ok' : 'error', saved ? t('hotkey.saved') : t('error.STORE'))
    stop()
  }

  shown.addEventListener('keydown', (event) => {
    if (!capturing) return
    event.preventDefault()
    if (event.key === 'Escape') {
      out.set('', '')
      stop()
      return
    }
    const result = acceleratorFromEvent(event)
    if (result.error === 'PARTIAL') return
    if (result.error === 'MODIFIER') return out.set('error', t('hotkey.needModifier'))
    if (result.error === 'KEY') return out.set('error', t('hotkey.unsupportedKey'))
    apply(result.accelerator)
  })
  shown.addEventListener('blur', () => { if (capturing) stop() })

  const change = h('button', {
    class: 'btn', type: 'button',
    onclick: () => {
      capturing = true
      out.set('', '')
      shown.value = t('hotkey.press')
      shown.focus()
    }
  }, t('hotkey.change'))

  return h('div', { class: 'field' },
    h('span', { class: 'label' }, t('hotkey.label')),
    h('div', { class: 'inline' },
      shown,
      change,
      h('button', { class: 'btn', type: 'button', onclick: () => apply('') }, t('hotkey.turnOff')),
      h('button', { class: 'btn', type: 'button', onclick: () => apply(DEFAULT_HOTKEY) }, t('hotkey.reset'))),
    h('span', { class: 'hint' }, t('hotkey.hint')),
    out)
}

function displaySection () {
  const monitorField = h('div', {})
  api.listMonitors().then((monitors) => {
    const options = monitors.map((m) => [
      m.index,
      t(m.primary ? 'display.monitor.primary' : 'display.monitor.item', { n: m.index + 1 })
    ])
    clear(monitorField).append(field(t('display.monitor'), select(options, settings.monitor, (v) => save({ monitor: Number(v) }))))
  }).catch(() => {})

  return h('section', {},
    h('h1', {}, t('display.title')),
    field(t('display.theme'), select(
      [['dark', t('display.theme.dark')], ['light', t('display.theme.light')], ['auto', t('display.theme.auto')]],
      settings.theme, (v) => save({ theme: v }))),
    field(t('display.language'), select(
      [['auto', t('display.language.auto')], ['en', 'English'], ['id', 'Bahasa Indonesia']],
      settings.language, (v) => save({ language: v }))),
    field(t('display.hoverDelay'), numberInput(settings.hoverDelayMs, { min: 0, max: 1000, step: 10 }, (v) => save({ hoverDelayMs: v }))),
    monitorField,
    field(t('display.offset'), numberInput(settings.offsetX, { min: -2000, max: 2000, step: 10 }, (v) => save({ offsetX: v }))),
    hotkeyControl())
}

/** The pill: what it shows at rest, and when a deadline takes it from the clock. */
function pillSection () {
  const urgent = Number(optionsOf('deadlines').urgentHours)
  const calendarOut = status()
  const calendar = h('input', {
    type: 'text', class: 'wide', autocomplete: 'off', spellcheck: 'false', placeholder: 'https://calendar.google.com/calendar/',
    value: optionsOf('deadlines').calendarUrl ?? '',
    onchange: (e) => {
      const url = e.target.value.trim()
      if (url && !/^https:\/\//i.test(url)) return calendarOut.set('error', t('pill.calendar.invalid'))
      calendarOut.set('', '')
      setOptions('deadlines', { calendarUrl: url || undefined })
    }
  })
  return h('section', {},
    h('h1', {}, t('pill.title')),
    h('p', {}, t('pill.intro')),
    field(t('pill.urgent'), numberInput(Number.isFinite(urgent) && urgent > 0 ? urgent : 5, { min: 0.5, max: 336, step: 0.5 }, (v) => {
      const hours = Number(v)
      setOptions('deadlines', { urgentHours: Number.isFinite(hours) && hours > 0 ? hours : undefined }).then(renderMain)
    }), t('pill.urgent.hint')),
    field(t('display.resting'), select(
      [['always', t('display.resting.always')], ['when-due', t('display.resting.when-due')], ['hover', t('display.resting.hover')]],
      settings.restingStyle, (v) => save({ restingStyle: v }))),
    field(t('display.dueHorizon'), numberInput(settings.dueHorizonHours, { min: 1, max: 336 }, (v) => save({ dueHorizonHours: v }))),
    field(t('pill.calendar'), calendar, t('pill.calendar.hint')),
    calendarOut)
}

function pagesSection () {
  const modules = hostModules()
  const move = (index, by) => {
    const next = [...modules]
    const [item] = next.splice(index, 1)
    next.splice(index + by, 0, item)
    saveHost(next).then(renderMain)
  }
  const list = h('ul', { class: 'list' })
  modules.forEach((m, i) => {
    list.append(h('li', {},
      h('label', { class: 'inline grow' },
        h('input', {
          type: 'checkbox', checked: m.enabled,
          onchange: (e) => saveHost(modules.map((x) => (x.id === m.id ? { ...x, enabled: e.target.checked } : x)))
        }),
        t(`pages.${m.id}`)),
      h('button', { class: 'btn', type: 'button', disabled: i === 0, 'aria-label': t('pages.up'), title: t('pages.up'), onclick: () => move(i, -1) }, '\u2191'),
      h('button', { class: 'btn', type: 'button', disabled: i === modules.length - 1, 'aria-label': t('pages.down'), title: t('pages.down'), onclick: () => move(i, 1) }, '\u2193')))
  })
  return h('section', {}, h('h1', {}, t('pages.title')), h('p', {}, t('pages.intro')), list)
}

function weatherSection () {
  const options = optionsOf('clock')
  const place = options.weather && Number.isFinite(options.weather.lat) && Number.isFinite(options.weather.lon) ? options.weather : null
  const units = options.weather?.units === 'f' ? 'f' : 'c'
  const keepWeather = (patch) => ({ ...(options.weather ?? {}), ...patch })
  const out = status()
  const results = h('ul', { class: 'list' })
  const query = h('input', {
    type: 'text', class: 'wide', maxlength: 80, autocomplete: 'off', placeholder: place?.name ?? '',
    onkeydown: (e) => { if (e.key === 'Enter') search() }
  })
  const searchBtn = h('button', { class: 'btn', type: 'button', onclick: search }, t('weather.search'))

  async function search () {
    const name = query.value.trim()
    if (!name) return
    clear(results)
    out.set('', t('weather.searching'))
    searchBtn.disabled = true
    try {
      const found = await api.weatherFind(name, language)
      if (!found.length) {
        out.set('error', t('weather.noResults'))
        return
      }
      out.set('', '')
      for (const p of found) {
        results.append(h('li', {},
          h('div', { class: 'grow' }, h('div', { class: 'name' }, p.label)),
          h('button', {
            class: 'btn primary', type: 'button',
            onclick: async () => {
              await setOptions('clock', { weather: keepWeather({ name: p.name, lat: p.lat, lon: p.lon }) })
              renderMain()
            }
          }, t('weather.use'))))
      }
    } catch {
      out.set('error', t('weather.failed'))
    } finally {
      searchBtn.disabled = false
    }
  }

  return h('section', {},
    h('h1', {}, t('weather.title')),
    h('p', {}, t('weather.intro')),
    h('p', {}, place ? t('weather.current', { name: place.name }) : t('weather.guess')),
    field(t('weather.city'), query),
    h('div', { class: 'inline' }, searchBtn, place && h('button', {
      class: 'btn', type: 'button',
      onclick: async () => {
        await setOptions('clock', { weather: units === 'f' ? { units: 'f' } : undefined })
        renderMain()
      }
    }, t('weather.clear'))),
    out,
    results,
    field(t('weather.units'), select([['c', t('weather.units.c')], ['f', t('weather.units.f')]], units,
      (v) => setOptions('clock', { weather: keepWeather({ units: v }) }))),
    field(t('weather.pane'), select([['weather', t('weather.pane.weather')], ['calendar', t('weather.pane.calendar')]],
      options.pane === 'calendar' ? 'calendar' : 'weather', (v) => setOptions('clock', { pane: v }))))
}

function musicSection () {
  const options = optionsOf('music')
  return h('section', {},
    h('h1', {}, t('music.title')),
    h('p', {}, t('music.intro')),
    h('label', { class: 'check' },
      h('input', { type: 'checkbox', checked: options.lyrics !== false, onchange: (e) => setOptions('music', { lyrics: e.target.checked }) }),
      t('music.lyrics')),
    h('p', { class: 'hint' }, t('music.lyrics.hint')),
    h('label', { class: 'check' },
      h('input', { type: 'checkbox', checked: options.peek !== false, onchange: (e) => setOptions('music', { peek: e.target.checked }) }),
      t('music.peek')))
}

function eventsSection () {
  const out = status()
  const title = h('input', { type: 'text', class: 'wide', maxlength: 120 })
  const date = h('input', { type: 'date' })
  const time = h('input', { type: 'time' })

  const list = h('ul', { class: 'list' })
  const sorted = [...localEvents].sort((a, b) => a.dueMs - b.dueMs)
  if (sorted.length === 0) list.append(h('li', { class: 'empty' }, t('events.empty')))
  for (const ev of sorted) {
    list.append(h('li', {},
      h('div', { class: 'grow' },
        h('div', { class: 'name' }, ev.title),
        h('div', { class: 'meta' }, dueLabel(ev.dueMs, Date.now(), language, t))),
      h('button', {
        class: 'btn danger', type: 'button',
        onclick: async () => {
          await saveLocalEvents(localEvents.filter((e) => e.id !== ev.id)).catch(() => {})
          renderAll()
        }
      }, t('events.delete'))))
  }

  return h('section', {},
    h('h1', {}, t('events.title')),
    h('p', {}, t('events.intro')),
    list,
    h('div', { class: 'card' },
      field(t('events.name'), title),
      h('div', { class: 'inline' }, field(t('events.date'), date), field(t('events.time'), time)),
      h('button', {
        class: 'btn primary', type: 'button',
        onclick: async () => {
          const res = normalizeLocalEvent({ title: title.value, date: date.value, time: time.value })
          if (!res.ok) {
            out.set('error', errorText(res.error))
            return
          }
          try {
            await saveLocalEvents([...localEvents, res.event])
          } catch {
            out.set('error', t('error.STORE'))
            return
          }
          renderAll()
        }
      }, t('events.add')),
      out))
}

function generalSection () {
  const version = h('p', {}, '')
  api.version().then((v) => { version.textContent = t('general.version', { v }) }).catch(() => {})

  const autostart = h('input', {
    type: 'checkbox',
    onchange: async (e) => {
      try {
        await (e.target.checked ? api.autostart.enable() : api.autostart.disable())
      } catch {
        e.target.checked = !e.target.checked
      }
    }
  })
  api.autostart.isEnabled().then((on) => { autostart.checked = on }).catch(() => {})

  const copy = async (text, out) => {
    try {
      await navigator.clipboard.writeText(text)
      out.set('ok', t('general.diagnostics.done'))
    } catch {
      out.set('error', t('general.diagnostics.failed'))
    }
  }

  const diagOut = status()
  const exportOut = status()
  const importOut = status()
  const importBox = h('textarea', {
    rows: 5, class: 'wide', placeholder: t('general.import.placeholder'), spellcheck: 'false'
  })

  return h('section', {},
    h('h1', {}, t('general.title')),
    h('label', { class: 'check' }, autostart, t('general.autostart')),

    h('h2', {}, t('general.export')),
    h('p', {}, t('general.export.hint')),
    h('button', {
      class: 'btn', type: 'button',
      onclick: () => copy(exportSettingsText(settings), exportOut)
    }, t('general.export')),
    exportOut,
    h('div', { class: 'field' }, importBox),
    h('button', {
      class: 'btn', type: 'button',
      onclick: async () => {
        const imported = parseImportedSettings(importBox.value)
        if (!imported) {
          importOut.set('error', t('general.import.invalid'))
          return
        }
        const ok = await save({ ...imported, feeds: settings.feeds, onboarded: true })
        if (ok) {
          importBox.value = ''
          importOut.set('ok', t('general.import.done'))
          renderMain()
        } else {
          importOut.set('error', t('error.STORE'))
        }
      }
    }, t('general.import.apply')),
    importOut,

    h('h2', {}, t('general.diagnostics')),
    h('p', {}, t('general.diagnostics.hint')),
    h('button', {
      class: 'btn', type: 'button',
      onclick: async () => {
        try {
          await copy(await api.diagnostics(), diagOut)
        } catch {
          diagOut.set('error', t('general.diagnostics.failed'))
        }
      }
    }, t('general.diagnostics')),
    diagOut,

    version,
    h('p', {}, t('general.privacy')))
}

/** Settings as shareable text. Feed rows (and so their links) are left out. */
function exportSettingsText (current) {
  const { feeds, onboarded, ...rest } = current
  return JSON.stringify({ app: 'deadline-notch', settings: rest }, null, 2)
}

function parseImportedSettings (text) {
  try {
    const parsed = JSON.parse(text)
    if (parsed?.app !== 'deadline-notch' || !parsed.settings || typeof parsed.settings !== 'object') return null
    return parsed.settings
  } catch {
    return null
  }
}

const BUILDERS = {
  feeds: feedsSection,
  notifications: notificationsSection,
  display: displaySection,
  events: eventsSection,
  pill: pillSection,
  pages: pagesSection,
  weather: weatherSection,
  music: musicSection,
  general: generalSection
}

// ------------------------------------------------------------------- layout

function renderNav () {
  clear(nav).append(
    h('p', { class: 'nav-title' }, t(`nav.group.${scope}`)),
    ...SCOPES[scope].map((name) => h('button', {
      type: 'button',
      'aria-current': name === section ? 'page' : null,
      onclick: () => { section = name; renderAll() }
    }, t(`nav.${name}`))))
}

/** Go to a section, which also picks the scope (deadlines or Blink) it belongs to. */
function openSection (name) {
  if (!(name in BUILDERS)) return
  section = name
  scope = scopeOf(name)
  document.title = t(`settings.title.${scope}`)
  renderAll()
}

function renderMain () {
  clear(main).append(BUILDERS[section]())
}

// ------------------------------------------------------------------- wizard

function renderWizard () {
  wizard.hidden = false
  const stepLabels = [t('onboarding.step1'), t('onboarding.step2'), t('onboarding.step3')]
  const steps = h('div', { class: 'steps' },
    stepLabels.map((label, i) => h('span', { 'aria-current': i + 1 === wizardStep ? 'step' : null }, `${i + 1}. ${label}`)))

  const finish = async () => {
    await save({ onboarded: true })
    wizard.hidden = true
    renderAll()
  }
  document.querySelector('.shell').inert = true

  let body
  if (wizardStep === 1) {
    const help = h('p', {}, wizardKind ? t(`feeds.help.${wizardKind}`) : t('onboarding.pickOne'))
    const buttons = FEED_KINDS.map((kind) => h('button', {
      class: 'choice', type: 'button', 'aria-pressed': String(kind === wizardKind),
      onclick: () => { wizardKind = kind; renderWizard() }
    }, t(`feeds.kind.${kind}`)))
    body = h('div', {},
      h('h1', {}, t('onboarding.step1')),
      h('div', { class: 'choices' }, buttons),
      help,
      h('div', { class: 'wizard-actions' },
        h('button', { class: 'btn', type: 'button', onclick: finish }, t('onboarding.skip')),
        h('button', {
          class: 'btn primary', type: 'button', disabled: !wizardKind,
          onclick: () => { wizardStep = 2; renderWizard() }
        }, t('onboarding.next'))))
  } else if (wizardStep === 2) {
    body = h('div', {},
      h('h1', {}, t('onboarding.step2')),
      addFeedForm({ kind: wizardKind, onSaved: () => { wizardStep = 3; renderWizard() } }),
      h('div', { class: 'wizard-actions' },
        h('button', { class: 'btn', type: 'button', onclick: () => { wizardStep = 1; renderWizard() } }, t('onboarding.back')),
        h('button', { class: 'btn', type: 'button', onclick: finish }, t('onboarding.skip'))))
  } else {
    body = h('div', {},
      h('h1', {}, t('onboarding.step3')),
      h('p', {}, t('onboarding.done')),
      h('div', { class: 'wizard-actions' },
        h('span', {}),
        h('button', { class: 'btn primary', type: 'button', onclick: finish }, t('onboarding.finish'))))
  }

  clear(wizard).append(h('div', { class: 'wizard-inner' },
    h('p', {}, t('onboarding.title')), steps, body))
}

function renderAll () {
  renderNav()
  renderMain()
  if (!settings.onboarded) renderWizard()
  // The wizard covers the page; keep keyboard and screen readers out of what is behind it.
  document.querySelector('.shell').inert = !wizard.hidden
}

// --------------------------------------------------------------------- init

async function init () {
  // The notch opens this on the part it was asked from: the deadlines' feeds, or Blink's general settings.
  if (typeof window.__SECTION__ === 'string' && window.__SECTION__ in BUILDERS) {
    section = window.__SECTION__
    scope = scopeOf(section)
  }
  window.__openSection = openSection
  const [rawSettings, events, rawHost] = await Promise.all([
    api.storeGet('settings').catch(() => null),
    api.storeGet('local_events').catch(() => null),
    api.storeGet('host').catch(() => null)
  ])
  settings = normalizeSettings(rawSettings)
  localEvents = Array.isArray(events) ? events : []
  host = rawHost && typeof rawHost === 'object' ? rawHost : { modules: [] }
  applyLanguage()
  renderAll()
}

init()
