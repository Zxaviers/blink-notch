// Debug-only sustained-animation test, loaded when DN_STRESS is set (see
// scripts/stress-cost.ps1). It opens the panel and then animates
// something every frame, to measure what a continuously moving module (music
// progress, lyrics, timer ring) would cost with the current WebView2 flags.
//
// Variants:
//   idle       panel open and still: the baseline
//   transform  a bar moved with transform (compositor-friendly)
//   layout     a bar resized with width (layout and paint every frame)
//   text       a counter whose text changes every frame
//   scroll     a tall list scrolled with transform (lyrics-like)

export function start (variant, { open, inner }) {
  open() // the real open state, so the shell keeps its size and clip consistent

  const box = document.createElement('div')
  box.style.cssText = 'position:absolute;inset:16px 12px 8px;overflow:hidden;color:#f2f2f2;font:13px monospace'
  inner.replaceChildren(box)

  const bar = document.createElement('div')
  bar.style.cssText = 'height:10px;background:#e0b25a;border-radius:5px;transform-origin:left;width:100%'
  const label = document.createElement('div')
  label.style.cssText = 'margin-top:12px;font-size:22px'
  const list = document.createElement('div')
  list.style.cssText = 'margin-top:12px;will-change:transform'
  for (let i = 0; i < 60; i++) {
    const line = document.createElement('div')
    line.textContent = `line ${i} the quick brown fox jumps over the lazy dog`
    line.style.cssText = 'padding:3px 0'
    list.append(line)
  }
  box.append(bar, label, list)

  if (variant === 'idle') return

  const t0 = performance.now()
  const frame = (now) => {
    const t = (now - t0) / 1000
    const p = (t % 4) / 4
    if (variant === 'transform') bar.style.transform = `scaleX(${p})`
    else if (variant === 'layout') bar.style.width = `${p * 100}%`
    else if (variant === 'text') label.textContent = `${(t % 100).toFixed(3)} s`
    else if (variant === 'scroll') list.style.transform = `translateY(${-(t * 40) % 1200}px)`
    requestAnimationFrame(frame)
  }
  requestAnimationFrame(frame)
}
