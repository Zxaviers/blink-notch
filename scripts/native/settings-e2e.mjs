// Gate G4: every settings flow end to end through the real notch pipe.
//
// Starts the real blink.exe (control pipe for observing it) and the real blink-settings.exe, and
// drives the settings PAGE through WebView2's debugging port (DOM clicks and key events, no real
// mouse or keyboard; the settings window stays hidden). A local calendar server stands in for a
// real feed.
//
//   node scripts/native/settings-e2e.mjs [releaseDir]
//
// Leaves nothing behind: scratch data folder, scratch Run key, and the feed's Credential Manager
// entry are removed at the end.

import { spawn, execFileSync } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.join(here, '..', '..')
const releaseDir = path.resolve(process.argv[2] ?? path.join(root, 'target', 'release'))
const BLINK = path.join(releaseDir, 'blink.exe')
const SETTINGS = path.join(releaseDir, 'blink-settings.exe')
const FIXTURE_PORT = 8793
const CDP_PORT = 9344
const RUN_KEY = 'Software\\BlinkTest\\RunG4'

let passed = 0
let failed = 0
const check = (what, ok, detail = '') => {
  if (ok) { passed++; console.log(`  ok    ${what}`) } else { failed++; console.log(`  FAIL  ${what}  ${detail}`) }
}
const section = (name) => console.log(name)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
async function until (fn, ms = 10000, step = 100) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    try { const v = await fn(); if (v) return v } catch { /* keep waiting */ }
    await sleep(step)
  }
  return null
}

// --------------------------------------------------------------- the control pipe
class Pipe {
  constructor (name) { this.name = name; this.buf = ''; this.waiters = [] }
  async connect () {
    this.sock = await until(() => new Promise((resolve, reject) => {
      const s = net.connect(`\\\\.\\pipe\\${this.name}`)
      s.once('connect', () => resolve(s))
      s.once('error', reject)
    }), 10000, 200)
    if (!this.sock) throw new Error(`cannot connect to ${this.name}`)
    this.sock.setEncoding('utf8')
    this.sock.on('data', (d) => {
      this.buf += d
      let at
      while ((at = this.buf.indexOf('\n')) >= 0) {
        const line = this.buf.slice(0, at); this.buf = this.buf.slice(at + 1)
        this.waiters.shift()?.(JSON.parse(line))
      }
    })
    return this
  }

  send (cmd) {
    return new Promise((resolve) => { this.waiters.push(resolve); this.sock.write(`${JSON.stringify(cmd)}\n`) })
  }

  close () { this.sock?.destroy() }
}
// The clock owns the pill when nothing is close, so ask for the deadlines page before reading its state.
const state = async (pipe) => { await pipe.send({ cmd: 'switch', to: 'deadlines' }); return pipe.send({ cmd: 'state' }) }

// ---------------------------------------------------------------- the page (CDP)
class Page {
  async connect () {
    const target = await until(async () => {
      const list = await (await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`)).json()
      return list.find((t) => t.type === 'page' && t.url.includes('settings.html'))
    }, 20000, 250)
    if (!target) throw new Error('no settings page on the debugging port')
    this.ws = new WebSocket(target.webSocketDebuggerUrl)
    await new Promise((resolve, reject) => { this.ws.onopen = resolve; this.ws.onerror = reject })
    this.id = 0
    this.pending = new Map()
    this.ws.onmessage = (e) => {
      const m = JSON.parse(e.data)
      if (m.id && this.pending.has(m.id)) { this.pending.get(m.id)(m); this.pending.delete(m.id) }
    }
    return this
  }

  async eval (expression) {
    const id = ++this.id
    const reply = new Promise((resolve) => this.pending.set(id, resolve))
    this.ws.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true } }))
    const m = await reply
    if (m.result?.exceptionDetails) throw new Error(m.result.exceptionDetails.exception?.description ?? 'page error')
    return m.result?.result?.value
  }

  close () { this.ws?.close() }
}

// ---------------------------------------------------------------------- helpers
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'blink-g4-'))
const dataDir = path.join(tmp, 'data')
fs.mkdirSync(dataDir, { recursive: true })
const readJson = (name) => { try { return JSON.parse(fs.readFileSync(path.join(dataDir, name), 'utf8')) } catch { return null } }
const procs = []
function run (exe, args, env) {
  const p = spawn(exe, args, { env: { ...process.env, ...env }, stdio: 'ignore', windowsHide: true })
  procs.push(p)
  return p
}
const ps = (script) => execFileSync('powershell', ['-NoProfile', '-Command', script], { encoding: 'utf8' }).trim()
// Close the settings window the way its X button does (WM_CLOSE), even though it is hidden.
const closeSettingsWindows = () => ps(`
Add-Type -TypeDefinition 'using System; using System.Text; using System.Runtime.InteropServices; public static class Closer { public delegate bool Proc(IntPtr h, IntPtr l); [DllImport("user32.dll")] public static extern bool EnumWindows(Proc p, IntPtr l); [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid); [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n); [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l); public static void CloseAll(uint pid) { EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); var sb = new StringBuilder(64); GetClassName(h, sb, 64); if (p == pid && sb.ToString() == "Tauri Window") PostMessage(h, 0x10, IntPtr.Zero, IntPtr.Zero); return true; }, IntPtr.Zero); } }'
foreach ($p in Get-Process -Name 'blink-settings' -ErrorAction SilentlyContinue) { [Closer]::CloseAll([uint32]$p.Id) }`)
const countProcs = (name) => Number(ps(`@(Get-Process -Name '${name}' -ErrorAction SilentlyContinue).Count`))
const credentialTargets = () => execFileSync('cmdkey', ['/list']).toString()

const baseEnv = {
  BLINK_DATA_DIR: dataDir,
  BLINK_TZ: 'Asia/Jakarta',
  BLINK_RUN_KEY: RUN_KEY,
  BLINK_SETTINGS_EXE: SETTINGS,
  BLINK_SETTINGS_TEST: '1',
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`
}

let fixture, blinkA, blinkB, pipeA, pipeB, page
const createdFeedIds = []

try {
  fixture = run('node', [path.join(here, '..', 'serve-fixture.mjs'), String(FIXTURE_PORT)], {})
  await sleep(800)

  section('the notch starts with nothing set up')
  blinkA = run(BLINK, [], { ...baseEnv, BLINK_CONTROL: 'g4', BLINK_IPC: 'g4' })
  pipeA = await new Pipe('blink-control-g4').connect()
  let s = await state(pipeA)
  check('no feeds yet', s.state.module.hasFeeds === false)

  section('opening settings (as the tray and footer do), with the first-run wizard')
  await pipeA.send({ cmd: 'open_settings' })
  page = await new Page().connect()
  const wizardUp = await until(() => page.eval("document.getElementById('wizard').hidden === false && document.querySelectorAll('.choice').length === 3"))
  check('the settings program opens and shows the wizard', wizardUp)
  check('it is a separate process', countProcs('blink-settings') === 1)

  await page.eval("document.querySelectorAll('.choice')[2].click()") // ICS link
  await page.eval("document.querySelector('.wizard-actions .btn.primary').click()")
  await until(() => page.eval("document.querySelectorAll('#wizard input.wide').length === 2"))
  await page.eval(`(() => {
    const [name, url] = document.querySelectorAll('#wizard input.wide')
    name.value = 'G4 fixture'
    url.value = 'http://127.0.0.1:${FIXTURE_PORT}/cal.ics'
    url.dispatchEvent(new Event('input', { bubbles: true }))
    ;[...document.querySelectorAll('#wizard .inline .btn')].find((b) => b.textContent === 'Test link').click()
  })()`)
  const tested = await until(() => page.eval("document.querySelector('#wizard p.status.ok')?.textContent ?? null"), 15000)
  check('"Test link" fetches and understands the calendar', Boolean(tested) && /5/.test(tested), String(tested))
  await page.eval("document.querySelector('#wizard .inline .btn.primary').click()")
  await until(() => page.eval("document.querySelector('#wizard h1')?.textContent === document.querySelectorAll('#wizard .steps span')[2].textContent.slice(3)"))
  await sleep(400)
  await page.eval("document.querySelector('#wizard .wizard-actions .btn.primary').click()") // Finish
  const settings1 = await until(() => { const j = readJson('settings.json'); return j?.feeds?.length === 1 && j.onboarded ? j : null })
  check('the feed is saved and the wizard is finished', Boolean(settings1))
  createdFeedIds.push(...(settings1?.feeds ?? []).map((f) => f.id))
  check('settings.json holds no link', !fs.readFileSync(path.join(dataDir, 'settings.json'), 'utf8').includes('127.0.0.1'))
  check('the link went to Credential Manager', createdFeedIds.length === 1 && credentialTargets().includes(`${createdFeedIds[0]}.deadline-notch`))
  const loaded = await until(async () => { const st = await state(pipeA); return st.state.module.upcoming.length === 4 ? st : null }, 15000)
  check('the running notch picked the feed up through the pipe and loaded its deadlines', Boolean(loaded), JSON.stringify((await state(pipeA)).state.module.upcoming))

  section('language change reaches the notch')
  const before = (await state(pipeA)).state.module.sync.text
  await page.eval("window.__openSection('display')")
  await sleep(300)
  await page.eval(`(() => {
    const sel = [...document.querySelectorAll('#main select')].find((s) => [...s.options].some((o) => o.value === 'id'))
    sel.value = 'id'; sel.dispatchEvent(new Event('change', { bubbles: true }))
  })()`)
  const after = await until(async () => { const t = (await state(pipeA)).state.module.sync.text; return t !== before ? t : null })
  check('the footer text changes language in the notch', Boolean(after) && !/^Synced/.test(after), `${before} -> ${after}`)
  check('settings.json says Indonesian', readJson('settings.json')?.language === 'id')

  section('hotkey rebind, including the in-use error')
  blinkB = run(BLINK, [], { ...baseEnv, BLINK_DATA_DIR: path.join(tmp, 'dataB'), BLINK_CONTROL: 'g4b', BLINK_IPC: 'g4b', BLINK_SETTINGS_EXE: '' })
  pipeB = await new Pipe('blink-control-g4b').connect()
  const held = await pipeB.send({ cmd: 'register_hotkey', accelerator: 'Ctrl+Alt+Shift+F7' })
  check('another process holds Ctrl+Alt+Shift+F7', held.hotkey === 'ok', JSON.stringify(held))
  const press = (key) => page.eval(`(() => {
    const change = [...document.querySelectorAll('#main .inline .btn')].find((b) => b.textContent.length > 0 && b.previousElementSibling?.classList.contains('hotkey-input'))
    change.click()
    const input = document.querySelector('.hotkey-input')
    input.dispatchEvent(new KeyboardEvent('keydown', { key: '${key}', code: '${key}', ctrlKey: true, altKey: true, shiftKey: true, bubbles: true, cancelable: true }))
  })()`)
  await press('F7')
  const busy = await until(() => page.eval("document.querySelector('#main p.status.error')?.textContent ?? null"))
  check('binding a key another program owns shows the error', Boolean(busy), String(busy))
  check('and nothing was saved', readJson('settings.json')?.hotkey !== 'Ctrl+Alt+Shift+F7')
  await press('F6')
  const good = await until(() => page.eval("document.querySelector('#main p.status.ok')?.textContent ?? null"))
  check('a free key is accepted', Boolean(good), String(good))
  const live = await until(async () => ((await state(pipeA)).window.hotkey === 'Ctrl+Alt+Shift+F6' ? true : null))
  check('the notch registered it', Boolean(live), (await state(pipeA)).window.hotkey)
  check('and settings.json has it', readJson('settings.json')?.hotkey === 'Ctrl+Alt+Shift+F6')

  section('the two settings scopes, pages and options reach the notch')
  const navNames = () => page.eval("[...document.querySelectorAll('#nav button')].map((b) => b.textContent)")
  check('Blink settings (the three dots) has its own sections, without the deadlines ones', (await navNames()).length === 5, JSON.stringify(await navNames()))
  check('and the pill options are not on the Display page', await page.eval("!document.querySelector('#main').textContent.includes('Always show the pill') && !document.querySelector('#main').textContent.includes('Selalu tampilkan pil')"))
  await page.eval("window.__openSection('pages')")
  await sleep(300)
  await page.eval("(() => { const c = document.querySelectorAll('#main li input[type=checkbox]')[4]; c.checked = false; c.dispatchEvent(new Event('change', { bubbles: true })) })()")
  const projectsOff = await until(async () => (await state(pipeA)).state.modules.find((m) => m.id === 'projects')?.enabled === false ? true : null)
  check('turning a page off in settings removes it from the notch', Boolean(projectsOff))
  await page.eval("window.__openSection('music')")
  await sleep(300)
  await page.eval("(() => { const c = document.querySelectorAll('#main input[type=checkbox]')[1]; c.checked = true; c.dispatchEvent(new Event('change', { bubbles: true })) })()")
  const keepSaved = await until(() => readJson('host.json')?.modules?.find((m) => m.id === 'music')?.options?.keepOpen === true ? true : null)
  check('keeping the notch open while a song plays is saved with the Music page options', Boolean(keepSaved))
  await page.eval("window.__openSection('display')")
  await sleep(300)
  await page.eval("(() => { const sel = [...document.querySelectorAll('#main select')].find((s) => [...s.options].some((o) => o.value === 'games')); sel.value = 'games'; sel.dispatchEvent(new Event('change', { bubbles: true })) })()")
  const fsSaved = await until(() => readJson('host.json')?.hideInFullscreen === 'games' ? true : null)
  check('what the notch does in a fullscreen app is saved in host.json', Boolean(fsSaved))
  await page.eval("window.__openSection('weather')")
  await sleep(300)
  await page.eval("(() => { const sel = [...document.querySelectorAll('#main select')].find((s) => [...s.options].some((o) => o.value === 'f')); sel.value = 'f'; sel.dispatchEvent(new Event('change', { bubbles: true })) })()")
  const unitsSaved = await until(() => readJson('host.json')?.modules?.find((m) => m.id === 'clock')?.options?.weather?.units === 'f' ? true : null)
  check('the weather units are saved with the Clock page options', Boolean(unitsSaved))
  await page.eval("window.__openSection('pill')")
  await sleep(300)
  check('the deadlines scope has its own four sections', (await navNames()).length === 4, JSON.stringify(await navNames()))
  await page.eval("(() => { const n = document.querySelector('#main input[type=number]'); n.value = '2'; n.dispatchEvent(new Event('change', { bubbles: true })) })()")
  const urgentSaved = await until(() => readJson('host.json')?.modules?.find((m) => m.id === 'deadlines')?.options?.urgentHours === 2 ? true : null)
  check('the hours before a deadline takes the pill are saved for the Deadlines page', Boolean(urgentSaved))
  check('and the pages saved before are kept', readJson('host.json')?.modules?.find((m) => m.id === 'projects')?.enabled === false)

  section('personal events')
  await page.eval("window.__openSection('events')")
  await sleep(300)
  await page.eval(`(() => {
    const d = new Date(Date.now() + 36 * 3600e3)
    const pad = (n) => String(n).padStart(2, '0')
    document.querySelector('#main input.wide').value = 'Dentist'
    document.querySelector('#main input[type=date]').value = d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate())
    document.querySelector('#main input[type=time]').value = '10:00'
    document.querySelector('#main .card .btn.primary').click()
  })()`)
  const dentist = await until(async () => ((await state(pipeA)).state.module.upcoming.includes('Dentist') ? true : null))
  check('an event added in settings appears in the notch', Boolean(dentist))
  check('local_events.json has it', readJson('local_events.json')?.some((e) => e.title === 'Dentist'))

  section('export, import, diagnostics, autostart')
  await page.eval("window.__openSection('general')")
  await sleep(300)
  await page.eval("window.__clip = []; navigator.clipboard.writeText = async (t) => { window.__clip.push(t) }")
  await page.eval("document.querySelectorAll('#main button.btn')[0].click()")
  const exported = await until(() => page.eval('window.__clip[0] ?? null'))
  check('export puts settings text on the clipboard', Boolean(exported) && JSON.parse(exported).app === 'deadline-notch')
  check('the export has no feeds and no links', !/127\.0\.0\.1|feeds/.test(exported ?? ''))
  const imported = JSON.stringify({ app: 'deadline-notch', settings: { theme: 'light', toast: false } })
  await page.eval(`document.querySelector('#main textarea').value = ${JSON.stringify(imported)}`)
  await page.eval("document.querySelectorAll('#main button.btn')[1].click()")
  const applied = await until(() => { const j = readJson('settings.json'); return j?.theme === 'light' && j.toast === false ? j : null })
  check('import applies settings and keeps the feed', Boolean(applied) && applied.feeds.length === 1)
  await page.eval("document.querySelectorAll('#main button.btn')[2].click()")
  const report = await until(() => page.eval('window.__clip[1] ?? null'))
  check('diagnostics covers settings and the running notch', /Blink 0\.2\.0/.test(report ?? '') && /Notch: running/.test(report ?? '') && /link_saved=true/.test(report ?? ''), String(report).slice(0, 200))
  check('and leaks no link, title or event name', !/127\.0\.0\.1|Problem Set|Dentist|G4 fixture/.test(report ?? ''))
  await page.eval("(() => { const c = document.querySelector('#main input[type=checkbox]'); c.checked = true; c.dispatchEvent(new Event('change', { bubbles: true })) })()")
  const runValue = await until(() => { try { return execFileSync('reg', ['query', `HKCU\\${RUN_KEY}`, '/v', 'Blink']).toString() } catch { return null } })
  check('autostart writes the notch (not the settings program) to the Run key', Boolean(runValue) && /blink\.exe/i.test(runValue) && !/blink-settings/i.test(runValue), String(runValue))
  await page.eval("(() => { const c = document.querySelector('#main input[type=checkbox]'); c.checked = false; c.dispatchEvent(new Event('change', { bubbles: true })) })()")
  const gone = await until(() => { try { execFileSync('reg', ['query', `HKCU\\${RUN_KEY}`, '/v', 'Blink'], { stdio: 'pipe' }); return null } catch { return true } })
  check('and turning it off removes it', Boolean(gone))

  section('second launch, then remove the feed')
  run(SETTINGS, [], baseEnv)
  await sleep(2500)
  check('a second launch does not start a second settings program', countProcs('blink-settings') === 1, `count=${countProcs('blink-settings')}`)
  await page.eval("window.__openSection('feeds')")
  await sleep(300)
  await page.eval("window.confirm = () => true; document.querySelector('#main .btn.danger').click()")
  const removed = await until(() => readJson('settings.json')?.feeds?.length === 0)
  check('removing the feed updates settings.json', Boolean(removed))
  check('and deletes its Credential Manager entry', !credentialTargets().includes(`${createdFeedIds[0]}.deadline-notch`))
  const onlyEvent = await until(async () => { const u = (await state(pipeA)).state.module.upcoming; return u.length === 1 && u[0] === 'Dentist' ? true : null }, 15000)
  check('the notch drops the feed but keeps the personal event', Boolean(onlyEvent))

  section('paste: one click checks the copied link and adds the deadlines')
  const keepClip = ps('try { $c = Get-Clipboard -Raw -ErrorAction Stop; if ($c) { $c } } catch { }')
  const clipIsText = keepClip.length > 0 // an empty or non-text clipboard is left alone: the test would have to overwrite it
  if (clipIsText) {
    ps(`Set-Clipboard -Value 'http://127.0.0.1:${FIXTURE_PORT}/cal.ics'`)
    await page.eval("window.__openSection('feeds')")
    await sleep(300)
    const hasBrone = await page.eval("[...document.querySelectorAll('#main .btn')].some((b) => b.textContent.length > 0 && !b.hidden && b.textContent.includes('BRONE'))")
    check('the form has a shortcut to the BRONE page for getting the link', Boolean(hasBrone))
    await page.eval("[...document.querySelectorAll('#main .inline .btn')].find((b) => b.textContent === 'Paste and add' || b.textContent === 'Tempel dan tambahkan').click()")
    const pasted = await until(() => { const j = readJson('settings.json'); return j?.feeds?.length === 1 ? j : null }, 20000)
    check('Paste and add tests the copied link and saves it as a feed', Boolean(pasted))
    createdFeedIds.push(...(pasted?.feeds ?? []).map((f) => f.id))
    const added = await until(async () => ((await state(pipeA)).state.module.upcoming.includes('Problem Set 4') ? true : null), 15000)
    check('and its deadlines appear in the notch', Boolean(added))
    await page.eval("window.confirm = () => true; document.querySelector('#main .btn.danger').click()")
    await until(() => readJson('settings.json')?.feeds?.length === 0)
    ps(`Set-Clipboard -Value @'
${keepClip}
'@`)
  } else {
    console.log('  skip  the clipboard is empty or holds something other than text; not touching it')
  }

  section('closing settings leaves no web engine behind')
  const openCount = ps("@(Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | Where-Object { $_.CommandLine -match 'dev.riski.blink' }).Count")
  check('while the window is open the web engine is running (so the check below can fail)', Number(openCount) > 0, `count=${openCount}`)
  page.close()
  closeSettingsWindows()
  const exited = await until(() => countProcs('blink-settings') === 0, 15000)
  check('the settings program exits when its window closes', Boolean(exited))
  await sleep(1500)
  const webview = ps("@(Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | Where-Object { $_.CommandLine -match 'dev.riski.blink' }).Count")
  check('no WebView2 processes remain', Number(webview) === 0, `count=${webview}`)
  const kids = ps(`@(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq ${blinkA.pid} }).Count`)
  check('the notch process has no child processes', Number(kids) === 0, `children=${kids}`)
  const mem = ps(`[math]::Round((Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $_.IDProcess -eq ${blinkA.pid} }).WorkingSetPrivate / 1MB, 2)`)
  console.log(`        (notch private memory after all this: ${mem} MB)`)

  section('settings on its own, with no notch running')
  await pipeA.send({ cmd: 'quit' }).catch(() => {})
  pipeA.close(); await sleep(800)
  run(SETTINGS, [], baseEnv)
  page = await new Page().connect()
  await until(() => page.eval("document.getElementById('main').children.length > 0"))
  const invalid = await page.eval("window.__TAURI__.core.invoke('set_hotkey', { accelerator: 'Banana+Z' }).then(() => 'ok', (e) => String(e))")
  check('an impossible hotkey is still rejected', invalid === 'HOTKEY_INVALID', invalid)
  const valid = await page.eval("window.__TAURI__.core.invoke('set_hotkey', { accelerator: 'Ctrl+Alt+F5' }).then(() => 'ok', (e) => String(e))")
  check('a valid one is accepted (the notch will register it when it starts)', valid === 'ok', valid)
  const diag = await page.eval("window.__TAURI__.core.invoke('diagnostics')")
  check('diagnostics says the notch is not running', /Notch: not running/.test(diag), diag.slice(-120))
  page.close()
  closeSettingsWindows()
  await until(() => countProcs('blink-settings') === 0, 15000)

  section('first run opens settings by itself')
  fs.rmSync(dataDir, { recursive: true, force: true }); fs.mkdirSync(dataDir, { recursive: true })
  const first = run(BLINK, [], { ...baseEnv, BLINK_IPC: 'g4c' }) // not a scripted run: no control pipe
  const opened = await until(() => countProcs('blink-settings') === 1, 15000)
  check('a notch with no feeds and no setup starts the settings program', Boolean(opened))
  closeSettingsWindows()
  await until(() => countProcs('blink-settings') === 0, 15000)
  first.kill()
} catch (e) {
  failed++
  console.log(`  FAIL  script error: ${e.stack ?? e}`)
} finally {
  try { await pipeB?.send({ cmd: 'quit' }) } catch { /* gone */ }
  pipeA?.close(); pipeB?.close()
  await sleep(500)
  for (const p of procs) { try { p.kill() } catch { /* gone */ } }
  try { ps(`Get-Process -Name 'blink-settings','blink' -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '${releaseDir}\*' } | Stop-Process -Force`) } catch { /* none */ }
  for (const id of createdFeedIds) { try { execFileSync('cmdkey', [`/delete:${id}.deadline-notch`], { stdio: 'ignore' }) } catch { /* already gone */ } }
  try { execFileSync('reg', ['delete', `HKCU\\${RUN_KEY}`, '/f'], { stdio: 'ignore' }) } catch { /* none */ }
  fs.rmSync(tmp, { recursive: true, force: true })
}
console.log(`\n${passed} passed, ${failed} failed`)
process.exit(failed > 0 ? 1 : 0)
