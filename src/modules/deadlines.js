// Deadlines module: the resting pill text and the expanded panel.
//
// Module contract (internal, PRD 10.3): an id, a compact view (`rest`), an
// expanded view (`panel`) and the events that may trigger a peek (the core's
// threshold notifications). The shell owns layout, animation and priority.

import { h } from '../dom.js'
import { urgencyOf, compactCountdown, dueLabel } from '../lib/countdown.js'
import { pillLabel, syncSummary } from '../lib/format.js'
import { isSnoozed, SNOOZE_KINDS } from '../lib/planner.js'

const MAX_UPCOMING_ROWS = 12
const MAX_OVERDUE_ROWS = 4

function countEl (task, ctx, { seconds = false } = {}) {
  return h('span', {
    class: 'count',
    dataset: { dueMs: task.dueMs, seconds: seconds ? '1' : '' }
  }, compactCountdown(task.dueMs, ctx.nowMs, ctx.t, { seconds }))
}

function actionButton (label, glyph, onClick) {
  return h('button', {
    class: 'act',
    type: 'button',
    title: label,
    'aria-label': label,
    onclick: (event) => { event.stopPropagation(); onClick() }
  }, glyph)
}

/** Due label, plus a marker while this task's reminders are snoozed. */
function whenText (task, ctx) {
  const base = dueLabel(task.dueMs, ctx.nowMs, ctx.language, ctx.t)
  if (!isSnoozed(ctx.snoozed, task.id, ctx.nowMs)) return base
  const until = new Date(ctx.snoozed[task.id])
  const time = `${String(until.getHours()).padStart(2, '0')}:${String(until.getMinutes()).padStart(2, '0')}`
  return `${base} · ⏰ ${time}`
}

function snoozeBar (task, ctx) {
  return h('div', { class: 'snooze-bar' },
    SNOOZE_KINDS.map((kind) => h('button', {
      class: 'chip', type: 'button',
      onclick: (event) => { event.stopPropagation(); ctx.actions.snooze(task.id, kind) }
    }, ctx.t(`snooze.${kind}`))),
    h('button', {
      class: 'chip', type: 'button',
      onclick: (event) => { event.stopPropagation(); ctx.actions.toggleSnooze(task.id) }
    }, ctx.t('snooze.cancel')))
}

function taskRow (task, ctx, { hero = false } = {}) {
  const urgency = urgencyOf(task.dueMs, ctx.nowMs)
  const label = task.source === 'local' ? ctx.t('row.local') : pillLabel(task, 12)
  const canOpen = Boolean(task.url)
  const canSnooze = urgency !== 'overdue'

  const actions = h('span', { class: 'actions' },
    canOpen && actionButton(ctx.t('row.open'), '↗', () => ctx.actions.openUrl(task.url)),
    canSnooze && actionButton(ctx.t('row.snooze'), '⏰', () => ctx.actions.toggleSnooze(task.id)),
    actionButton(ctx.t('row.done'), '✓', () => ctx.actions.markDone(task.id))
  )

  return h('li', {
    class: `row ${urgency}${hero ? ' hero' : ''}${canOpen ? ' linked' : ''}`,
    dataset: { id: task.id },
    onclick: canOpen ? () => ctx.actions.openUrl(task.url) : null
  },
  hero
    ? h('div', { class: 'hero-main' },
      h('span', { class: 'code' }, task.course || label),
      h('span', { class: 'title', title: task.title }, task.title),
      h('span', { class: 'hero-time' },
        countEl(task, ctx, { seconds: true }),
        h('span', { class: 'when' }, whenText(task, ctx))
      ))
    : [
        h('span', { class: 'code' }, label),
        h('span', { class: 'title', title: task.title }, task.title),
        h('span', { class: 'time' },
          countEl(task, ctx),
          h('span', { class: 'when' }, whenText(task, ctx))
        )
      ],
  actions,
  ctx.snoozeFor === task.id && snoozeBar(task, ctx))
}

function doneRow (task, ctx) {
  return h('li', { class: 'row done-row', dataset: { id: task.id } },
    h('span', { class: 'code' }, task.source === 'local' ? ctx.t('row.local') : pillLabel(task, 12)),
    h('span', { class: 'title', title: task.title }, task.title),
    h('span', { class: 'actions always' },
      actionButton(ctx.t('row.restore'), '↶', () => ctx.actions.restore(task.id))
    )
  )
}

function footer (ctx) {
  const summary = syncSummary(ctx.feeds, ctx.t, ctx.language)
  return h('div', { class: 'footer' },
    h('span', { class: `sync ${summary.kind}` }, summary.text),
    h('span', { class: 'footer-actions' },
      ctx.hasFeeds && actionButton(ctx.t('footer.refresh'), '↻', () => ctx.actions.refresh()),
      actionButton(ctx.t('footer.settings'), '⚙', () => ctx.actions.openSettings())
    )
  )
}

export const deadlinesModule = {
  id: 'deadlines',

  /** Resting pill content. */
  rest (ctx) {
    const hero = ctx.view.hero
    if (!hero) {
      return {
        label: '',
        count: ctx.hasFeeds ? ctx.t('rest.none') : ctx.t('rest.noFeeds'),
        urgency: 'later',
        dueMs: null
      }
    }
    return {
      label: pillLabel(hero, 12),
      count: compactCountdown(hero.dueMs, ctx.nowMs, ctx.t, { seconds: true }),
      urgency: urgencyOf(hero.dueMs, ctx.nowMs),
      dueMs: hero.dueMs
    }
  },

  /** Peek content for a task that just crossed a threshold. */
  peek (task, ctx) {
    return h('div', { class: `peek-body ${urgencyOf(task.dueMs, ctx.nowMs)}` },
      h('span', { class: 'code' }, task.course || pillLabel(task, 12)),
      h('span', { class: 'title' }, task.title),
      countEl(task, ctx, { seconds: true })
    )
  },

  /** Expanded panel. A fresh element each render; the shell swaps it in. */
  panel (ctx) {
    const { upcoming, overdue, doneList, hero } = ctx.view
    const root = h('div', { class: 'deadlines' })

    if (!ctx.hasFeeds && upcoming.length === 0 && overdue.length === 0) {
      root.append(h('div', { class: 'empty' },
        h('p', {}, ctx.t('list.noFeeds')),
        h('button', { class: 'cta', type: 'button', onclick: () => ctx.actions.openSettings() }, ctx.t('list.addFeed'))
      ))
    } else if (upcoming.length === 0 && overdue.length === 0) {
      root.append(h('div', { class: 'empty' }, h('p', {}, ctx.t('list.empty'))))
    } else {
      const list = h('ul', { class: 'rows' })
      const rest = hero && upcoming[0] === hero ? upcoming.slice(1) : upcoming
      if (hero && upcoming[0] === hero) list.append(taskRow(hero, ctx, { hero: true }))
      for (const task of rest.slice(0, MAX_UPCOMING_ROWS)) list.append(taskRow(task, ctx))
      root.append(list)

      if (overdue.length > 0) {
        const late = h('ul', { class: 'rows overdue-rows' })
        const heroIsOverdue = hero && overdue[0] === hero
        overdue.slice(0, MAX_OVERDUE_ROWS).forEach((task, i) => {
          late.append(taskRow(task, ctx, { hero: heroIsOverdue && i === 0 }))
        })
        root.append(h('div', { class: 'section-title overdue' }, ctx.t('section.overdue')), late)
      }
    }

    if (doneList.length > 0) {
      root.append(h('button', {
        class: 'section-title toggle',
        type: 'button',
        'aria-expanded': String(ctx.doneOpen),
        onclick: () => ctx.actions.toggleDone()
      }, ctx.t('section.done', { n: doneList.length })))
      if (ctx.doneOpen) {
        root.append(h('ul', { class: 'rows done-rows' }, doneList.map((task) => doneRow(task, ctx))))
      }
    }

    root.append(footer(ctx))
    return root
  },

  /** Refresh just the countdown texts in place (cheap per-second update). */
  tickPanel (container, ctx) {
    for (const el of container.querySelectorAll('.count[data-due-ms]')) {
      el.textContent = compactCountdown(Number(el.dataset.dueMs), ctx.nowMs, ctx.t, { seconds: el.dataset.seconds === '1' })
    }
  }
}
