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
    hotkey: await lib('hotkey'), settings: await lib('settings')
  }
  // fn name -> [module, export]
  const where = {
    normalizeThresholdsHours: 'notifyConfig', isValidFeedId: 'feeds', normalizeFeedKind: 'feeds', normalizeFeed: 'feeds',
    feedMetaFrom: 'feeds', upsertFeedMeta: 'feeds', clampInterval: 'feedUrl', describeUrl: 'feedUrl', validateUrl: 'feedUrl',
    nextRefreshDelayMs: 'feedUrl', inspectMoodleUrl: 'feedUrl', isValidAccelerator: 'hotkey', normalizeSettings: 'settings'
  }

  const out = OPS.map(({ fn, args }) => {
    const f = m[where[fn]]?.[fn]
    if (!f) return { error: `no such function ${fn}` }
    try {
      return { value: f(...args) }
    } catch (error) {
      return { error: error.code ?? error.message }
    }
  })
  fs.writeFileSync(path.join(fixtures, `ops-expected.${zone.replace('/', '_')}.json`), JSON.stringify(out, null, 1))
}
