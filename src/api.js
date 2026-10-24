// Thin wrapper over the Tauri bridge. The pages have no network access of
// their own (CSP default-src 'self'); everything with side effects goes
// through these commands.

const { invoke } = window.__TAURI__.core
const { listen, emit } = window.__TAURI__.event

export const api = {
  storeGet: (name) => invoke('store_get', { name }),
  storeSet: (name, value) => invoke('store_set', { name, value }),

  feedFetch: (id, force = false) => invoke('feed_fetch', { id, force }),
  feedTest: (url) => invoke('feed_test', { url }),
  feedSave: (id, url) => invoke('feed_save', { id, url }),
  feedRemove: (id) => invoke('feed_remove', { id }),
  feedHasSecret: (id) => invoke('feed_has_secret', { id }),

  notify: (title, body) => invoke('notify', { title, body }),
  openUrl: (url) => invoke('open_url', { url }),
  openSettings: () => invoke('open_settings'),
  diagnostics: () => invoke('diagnostics'),
  clipboardText: () => invoke('clipboard_text'),
  weatherFind: (name, language) => invoke('weather_find', { name, language }),

  listMonitors: () => invoke('list_monitors'),
  setPlacement: (monitor, offsetX) => invoke('set_placement', { monitor, offsetX }),
  setShape: (x, y, w, h) => invoke('set_shape', { x, y, w, h }),
  setTrayLabels: (labels) => invoke('set_tray_labels', labels),
  debugLog: (msg) => invoke('debug_log', { msg }),
  debugFlags: () => invoke('debug_flags'),

  setHotkey: (accelerator) => invoke('set_hotkey', { accelerator }),
  enterKeyboardMode: () => invoke('enter_keyboard_mode'),
  exitKeyboardMode: () => invoke('exit_keyboard_mode'),

  // The native settings program (blink-settings) sets __BLINK_SETTINGS__ and has its own
  // commands; the WebView2 app used the autostart plugin. The plugin path goes at cutover.
  autostart: window.__BLINK_SETTINGS__
    ? {
        isEnabled: () => invoke('autostart_is_enabled'),
        enable: () => invoke('autostart_enable'),
        disable: () => invoke('autostart_disable')
      }
    : {
        isEnabled: () => invoke('plugin:autostart|is_enabled'),
        enable: () => invoke('plugin:autostart|enable'),
        disable: () => invoke('plugin:autostart|disable')
      },

  version: () => window.__TAURI__.app.getVersion(),

  on: (event, handler) => listen(event, (e) => handler(e.payload)),
  emit: (event, payload) => emit(event, payload)
}
