// Generates the expected output of the "ops" cases from the JavaScript modules.
//   node crates/blink-core/tests/diff/gen-ops.mjs
// Runs under each zone in ZONES (some functions depend on the machine zone).

import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const fixtures = path.join(here, '..', 'fixtures')
const lib = (name) => import(`../../../../src/lib/${name}.js`)
export const ZONES = ['Asia/Jakarta', 'America/New_York', 'Europe/London', 'Australia/Lord_Howe']

const zoneArg = process.argv.indexOf('--zone')
if (zoneArg === -1) {
  fs.mkdirSync(fixtures, { recursive: true })
  const { OPS } = await import('./ops-cases.mjs')
  fs.writeFileSync(path.join(fixtures, 'ops-cases.json'), JSON.stringify(OPS, null, 1))
  for (const zone of ZONES) {
    const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), '--zone', zone], {
      env: { ...process.env, TZ: zone }, stdio: 'inherit'
    })
    if (r.status !== 0) process.exit(r.status ?? 1)
  }
  console.log(`wrote ${OPS.length} ops for ${ZONES.length} zones`)
} else {
  const zone = process.argv[zoneArg + 1]
  if (Intl.DateTimeFormat().resolvedOptions().timeZone !== zone) throw new Error('zone not applied')
  const { OPS } = await import('./ops-cases.mjs')

  const m = {
    notifyConfig: await lib('notifyConfig'), feeds: await lib('feeds'), feedUrl: await lib('feedUrl'),
    hotkey: await lib('hotkey'), settings: await lib('settings'), multiFetch: await lib('multiFetch'),
    localEvents: await lib('localEvents'), doneStore: await lib('doneStore'), planner: await lib('planner'), view: await lib('view'),
    i18n: await lib('i18n'), countdown: await lib('countdown'), format: await lib('format')
  }

  // Functions whose JS signature takes a translator get a language code instead.
  const T = (lang) => m.i18n.createT(lang)
  const adapters = {
    t: (lang, key, vars) => T(lang)(key, vars),
    resolveLanguage: (s, n) => m.i18n.resolveLanguage(s, n),
    compactCountdown: (due, now, lang, opts) => m.countdown.compactCountdown(due, now, T(lang), opts),
    dueLabel: (due, now, lang) => m.countdown.dueLabel(due, now, lang, T(lang)),
    syncSummary: (feeds, lang) => m.format.syncSummary(feeds, T(lang), lang),
    urgencyOf: (...a) => m.countdown.urgencyOf(...a),
    isDataStale: (...a) => m.countdown.isDataStale(...a),
    nextTickMs: (...a) => m.countdown.nextTickMs(...a),
    truncate: (...a) => m.format.truncate(...a),
    pillLabel: (...a) => m.format.pillLabel(...a)
  }
  // fn name -> [module, export]
  const where = {
    normalizeThresholdsHours: 'notifyConfig', isValidFeedId: 'feeds', normalizeFeedKind: 'feeds', normalizeFeed: 'feeds',
    feedMetaFrom: 'feeds', upsertFeedMeta: 'feeds', clampInterval: 'feedUrl', describeUrl: 'feedUrl', validateUrl: 'feedUrl',
    nextRefreshDelayMs: 'feedUrl', inspectMoodleUrl: 'feedUrl', isValidAccelerator: 'hotkey', normalizeSettings: 'settings',
    mergeFeedTasks: 'multiFetch', normalizeLocalEvent: 'localEvents', localTaskFrom: 'localEvents', withLocalTasks: 'localEvents',
    planNotifications: 'planner', recordFired: 'planner', pruneFired: 'planner', groupNotifications: 'planner', snoozeUntil: 'planner',
    isSnoozed: 'planner', expireSnoozes: 'planner', buildView: 'view', restModeFor: 'view'
  }

  // A stateful store driven by a list of steps (the done store is an object, not a function).
  const doneScript = (initial, steps) => {
    const store = m.doneStore.createDoneStore(initial)
    return steps.map((s) => {
      switch (s.op) {
        case 'isDone': return store.isDone(s.id)
        case 'mark': return store.mark(s.id, s.iso) ?? null
        case 'unmark': return store.unmark(s.id) ?? null
        case 'visible': return store.visible(s.tasks)
        case 'doneList': return store.doneList(s.tasks)
        case 'prune': return Object.fromEntries(store.prune(s.tasks, s.keepMs, s.nowMs))
        case 'lastAction': return store.lastAction()
        case 'undo': return store.undo() ?? null
        case 'toJSON': return store.toJSON()
        default: throw new Error('unknown step ' + s.op)
      }
    })
  }

  // The event id is random (local-<time>-<6 chars>): check its shape, then compare the rest.
  const maskId = (r) => {
    if (r && r.ok && r.event && /^local-[0-9a-z]+-[0-9a-z]{6}$/.test(r.event.id)) r.event.id = 'local-ID'
    return r
  }

  // {$localWall: 'YYYY-MM-DDTHH:mm:ss', plusMs} means that wall time in the machine zone, plus an offset.
  const resolve = (v) => (v && typeof v === 'object' && v.$localWall ? new Date(v.$localWall).getTime() + v.plusMs : v)

  const out = OPS.map(({ fn, args: rawArgs }) => {
    const args = rawArgs.map(resolve)
    const f = fn === 'doneScript' ? doneScript : adapters[fn] ?? m[where[fn]]?.[fn]
    if (!f) return { error: `no such function ${fn}` }
    try {
      const v = f(...args)
      return { value: fn === 'normalizeLocalEvent' ? maskId(v) : v }
    } catch (error) {
      return { error: error.code ?? error.message }
    }
  })
  fs.writeFileSync(path.join(fixtures, `ops-expected.${zone.replace('/', '_')}.json`), JSON.stringify(out, null, 1))
}
