// Generates the ICS differential fixtures from the JavaScript implementation.
//   node crates/blink-core/tests/diff/gen-ics.mjs
// Writes tests/fixtures/ics-cases.json (inputs) and ics-expected.<zone>.json
// (what parseTasks returns with the machine zone set to <zone>). The Rust test
// must reproduce every expected file exactly. The JS library stays the oracle
// until it is retired.

import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const fixtures = path.join(here, '..', 'fixtures')
export const ZONES = ['Asia/Jakarta', 'America/New_York', 'Europe/London', 'Australia/Lord_Howe']

const zoneArg = process.argv.indexOf('--zone')
if (zoneArg === -1) {
  // Parent: run one child per zone so Date uses that machine zone from the start.
  fs.mkdirSync(fixtures, { recursive: true })
  const { CASES } = await import('./ics-cases.mjs')
  fs.writeFileSync(path.join(fixtures, 'ics-cases.json'), JSON.stringify(CASES, null, 1))
  fs.writeFileSync(path.join(fixtures, 'ics-zones.json'), JSON.stringify(ZONES))
  for (const zone of ZONES) {
    const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), '--zone', zone], {
      env: { ...process.env, TZ: zone }, stdio: 'inherit'
    })
    if (r.status !== 0) process.exit(r.status ?? 1)
  }
  console.log(`wrote ${CASES.length} cases for ${ZONES.length} zones`)
} else {
  const zone = process.argv[zoneArg + 1]
  const { parseTasks } = await import('../../../../src/lib/parseTasks.js')
  const { CASES } = await import('./ics-cases.mjs')
  const resolved = Intl.DateTimeFormat().resolvedOptions().timeZone
  if (resolved !== zone) throw new Error(`expected machine zone ${zone}, got ${resolved}`)
  const out = CASES.map((c) => {
    const options = { now: new Date(c.nowMs) }
    if (c.keepOverdueMs !== undefined) options.keepOverdueMs = c.keepOverdueMs
    if (c.horizonMs !== undefined) options.horizonMs = c.horizonMs
    if (c.limit !== undefined) options.limit = c.limit
    try {
      return { tasks: parseTasks(c.ics, options) }
    } catch (error) {
      return { error: error.code ?? error.message }
    }
  })
  fs.writeFileSync(path.join(fixtures, `ics-expected.${zone.replace('/', '_')}.json`), JSON.stringify(out, null, 1))
}
