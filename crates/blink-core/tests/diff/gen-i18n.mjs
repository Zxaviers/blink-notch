// One-time import of the translation tables from the JavaScript source into
// crates/blink-core/src/i18n_data.rs.
//   node crates/blink-core/tests/diff/gen-i18n.mjs
// After the cutover the Rust file is the source of truth and this is not re-run.

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { STRINGS } from '../../../../src/lib/i18n.js'

const here = path.dirname(fileURLToPath(import.meta.url))
const out = path.join(here, '..', '..', 'src', 'i18n_data.rs')

const literal = (s) => {
  const j = JSON.stringify(s)
  if (/\\u/.test(j)) throw new Error(`unsupported escape in ${j}`)
  return j
}
const table = (name, strings) => {
  const rows = Object.entries(strings).map(([k, v]) => `    (${literal(k)}, ${literal(typeof v === 'function' ? '@@fn@@' : v)}),`)
  return `pub static ${name}: &[(&str, &str)] = &[\n${rows.join('\n')}\n];\n`
}

const header = `//! Translation tables, imported from src/lib/i18n.js by tests/diff/gen-i18n.mjs.
//! A value of "@@fn@@" marks an entry that is a function in the JS (plurals);
//! see i18n.rs. Generated once; after cutover this file is edited by hand.

`
fs.writeFileSync(out, header + table('EN', STRINGS.en) + '\n' + table('ID', STRINGS.id))
console.log(`wrote ${Object.keys(STRINGS.en).length} keys x 2 languages`)