// Browser harness for the pages: serves src/ over http with a stub of the
// Tauri bridge, so the notch and settings UI can be driven in an ordinary
// browser tab (including the Browser pane) without the native shell.
//   node scripts/ui-harness.mjs [port]
//   http://127.0.0.1:<port>/index.html      the notch (use window.__hover(true))
//   http://127.0.0.1:<port>/settings.html   the settings window
// State lives in localStorage; the tabs talk through a BroadcastChannel, the
// way the real windows talk through Tauri events. Feeds are fetched by the
// page from /cal.ics (a fixture with deadlines relative to now).

import http from 'node:http'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const srcDir = path.join(here, '..', 'src')
const port = Number(process.argv[2]) || 8770
const H = 3600 * 1000

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json' }

const stamp = (ms) => new Date(ms).toISOString().replace(/[-:]/g, '').replace(/\.\d{3}/, '')
function ics () {
  const now = Date.now()
  const events = [
    ['t1', 'Problem Set 4 is due', 'CS101 Algorithms', now + 3 * H + 14 * 60 * 1000],
    ['t2', 'Quiz 3 closes', 'MA201 Linear Algebra', now + 30 * H],
    ['t3', 'Lab report is due', 'PH110 Physics Lab', now + 4 * 24 * H],
    ['t4', 'Essay draft is due', 'EN150 Writing', now - 5 * H],
    ['t5', 'Project proposal is due', 'Basis Data', now + 11 * 24 * H]
  ]
  const lines = ['BEGIN:VCALENDAR', 'VERSION:2.0', 'PRODID:-//Deadline Notch harness//EN']
  for (const [uid, summary, course, due] of events) {
    lines.push('BEGIN:VEVENT', `UID:${uid}@harness.local`, `SUMMARY:${summary}`, `DTSTART:${stamp(due)}`,
      `CATEGORIES:${course}`, `URL:https://example.edu/mod/assign/view.php?id=${uid}`, 'END:VEVENT')
  }
  lines.push('END:VCALENDAR')
  return `${lines.join('\r\n')}\r\n`
}

const STUB = `
(() => {
  const chan = new BroadcastChannel('dn')
  const listeners = {}
  const dispatch = (event, payload) => (listeners[event] || []).forEach((fn) => fn({ payload }))
  chan.onmessage = (m) => dispatch(m.data.event, m.data.payload)
  const get = (k, d) => { try { return JSON.parse(localStorage.getItem('dn:' + k)) ?? d } catch { return d } }
  const set = (k, v) => localStorage.setItem('dn:' + k, JSON.stringify(v))
  const log = (...a) => { (window.__log = window.__log || []).push(a); console.log('[stub]', ...a) }

  const handlers = {
    store_get: ({ name }) => get('store:' + name, null),
    store_set: ({ name, value }) => { set('store:' + name, value) },
    feed_test: async ({ url }) => {
      try {
        const res = await fetch(url)
        if (!res.ok) return { ics: null, error: { code: 'HTTP_ERROR', transport: false } }
        const text = await res.text()
        if (!text.includes('BEGIN:VCALENDAR')) return { ics: null, error: { code: 'NOT_CALENDAR', transport: false } }
        return { ics: text, fromCache: false, fetchedAtMs: Date.now(), error: null }
      } catch { return { ics: null, error: { code: 'NETWORK', transport: true } } }
    },
    feed_save: ({ id, url }) => { const s = get('secrets', {}); s[id] = url; set('secrets', s) },
    feed_remove: ({ id }) => { const s = get('secrets', {}); delete s[id]; set('secrets', s) },
    feed_has_secret: ({ id }) => Boolean(get('secrets', {})[id]),
    feed_fetch: async ({ id }) => {
      const url = get('secrets', {})[id]
      if (!url) return { ics: null, error: { code: 'NO_SECRET', transport: false } }
      return handlers.feed_test({ url })
    },
    notify: ({ title, body }) => log('notify', title, body),
    open_url: ({ url }) => log('open_url', url),
    open_settings: () => log('open_settings'),
    diagnostics: () => 'Deadline Notch 0.1.0-harness\\nOS: browser',
    list_monitors: () => [{ index: 0, width: 1920, height: 1080, primary: true }, { index: 1, width: 1280, height: 720, primary: false }],
    set_placement: (a) => log('set_placement', a),
    set_shape: (a) => { window.__shape = a },
    set_tray_labels: (a) => log('tray', a),
    // Ctrl+Alt+X plays "taken by another app" so the error path can be driven.
    set_hotkey: ({ accelerator }) => {
      if (accelerator === 'Ctrl+Alt+X') throw 'HOTKEY_IN_USE'
      set('hotkey', accelerator); log('set_hotkey', accelerator)
    },
    enter_keyboard_mode: () => { log('enter_keyboard_mode'); return true },
    exit_keyboard_mode: () => { log('exit_keyboard_mode') },
    debug_log: () => {},
    'plugin:autostart|is_enabled': () => get('autostart', false),
    'plugin:autostart|enable': () => set('autostart', true),
    'plugin:autostart|disable': () => set('autostart', false)
  }

  window.__TAURI__ = {
    core: { invoke: async (cmd, args = {}) => {
      const fn = handlers[cmd]
      if (!fn) throw 'UNKNOWN_COMMAND:' + cmd
      return fn(args)
    } },
    event: {
      listen: async (event, cb) => { (listeners[event] = listeners[event] || []).push(cb); return () => {} },
      emit: async (event, payload) => { chan.postMessage({ event, payload }) }
    },
    app: { getVersion: async () => '0.1.0-harness' }
  }
  window.__hover = (inside) => dispatch('notch-hover', inside)
  window.__hotkey = () => dispatch('hotkey', null)
  window.__reset = () => { Object.keys(localStorage).filter((k) => k.startsWith('dn:')).forEach((k) => localStorage.removeItem(k)) }
})()
`

http.createServer((req, res) => {
  const url = new URL(req.url, 'http://x')
  if (url.pathname === '/__stub.js') {
    res.writeHead(200, { 'Content-Type': 'text/javascript' })
    return res.end(STUB)
  }
  if (url.pathname === '/cal.ics') {
    res.writeHead(200, { 'Content-Type': 'text/calendar', 'Access-Control-Allow-Origin': '*' })
    return res.end(ics())
  }
  const rel = url.pathname === '/' ? '/index.html' : url.pathname
  const file = path.normalize(path.join(srcDir, rel))
  if (!file.startsWith(srcDir) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    res.writeHead(404)
    return res.end('not found')
  }
  let body = fs.readFileSync(file)
  if (file.endsWith('.html')) body = Buffer.from(body.toString().replace('<script', '<script src="/__stub.js"></script>\n    <script'))
  res.writeHead(200, { 'Content-Type': TYPES[path.extname(file)] || 'application/octet-stream', 'Cache-Control': 'no-store' })
  res.end(body)
}).listen(port, '127.0.0.1', () => console.log(`harness on http://127.0.0.1:${port}/index.html and /settings.html`))
