import type { FrontendEvent, FrontendEventBatch } from '@nyanpasu/rpc/types'
import { fingerprint } from './fingerprint'
import type { EventDraft } from './normalize'

const DEDUPE_WINDOW_MS = 60_000
const RATE_WINDOW_MS = 60_000
const MAX_DISTINCT_PER_RATE_WINDOW = 60
const BATCH_SIZE = 16
const FLUSH_DELAY_MS = 1_000

export type ReporterOptions = {
  send: (batch: FrontendEventBatch) => Promise<void>
  now: () => number
  route: () => string
  /** Runs `callback` after `ms`; returns its cancellation. */
  schedule: (callback: () => void, ms: number) => () => void
}

export type Reporter = {
  capture: (draft: EventDraft) => void
  flush: () => void
  dispose: () => void
}

/**
 * One fingerprint's dedupe window. `open` accumulates repeats until it is
 * sent: the first occurrence is queued at once; a repeat after that is held
 * until the window ends.
 */
type DedupeWindow = {
  start: number
  open: FrontendEvent | null
  held: boolean
}

export function createReporter(options: ReporterOptions): Reporter {
  const windows = new Map<string, DedupeWindow>()
  // Held repeats whose window ended before the timer flushed them.
  const ready: FrontendEvent[] = []
  let queued = 0
  let dropped = 0
  let rateStart = -Infinity
  let rateCount = 0
  let cancelTimer: (() => void) | null = null
  let timerDeadline = Infinity
  // Set while the reporter itself runs, so logging it triggers is not captured.
  let busy = false

  const schedule = (deadline: number) => {
    if (deadline >= timerDeadline) return
    cancelTimer?.()
    timerDeadline = deadline
    cancelTimer = options.schedule(
      () => {
        cancelTimer = null
        timerDeadline = Infinity
        flush()
      },
      Math.max(0, deadline - options.now()),
    )
  }

  const createEvent = (
    draft: EventDraft,
    id: string,
    time: number,
  ): FrontendEvent => ({
    ...draft,
    fingerprint: id,
    count: 1,
    first_seen_ms: time,
    last_seen_ms: time,
    route: options.route(),
  })

  const capture = (draft: EventDraft) => {
    if (busy) return
    busy = true

    try {
      const time = options.now()
      const id = fingerprint(draft)
      const current = windows.get(id)

      if (current && time - current.start < DEDUPE_WINDOW_MS) {
        if (current.open) {
          current.open.count += 1
          current.open.last_seen_ms = time
        } else {
          current.open = createEvent(draft, id, time)
          current.held = true
          schedule(current.start + DEDUPE_WINDOW_MS)
        }
        return
      }

      if (time - rateStart >= RATE_WINDOW_MS) {
        rateStart = time
        rateCount = 0
      }
      if (rateCount >= MAX_DISTINCT_PER_RATE_WINDOW) {
        dropped += 1
        schedule(time + FLUSH_DELAY_MS)
        return
      }
      rateCount += 1

      if (current?.open) ready.push(current.open)
      windows.delete(id)
      windows.set(id, {
        start: time,
        open: createEvent(draft, id, time),
        held: false,
      })
      queued += 1

      if (queued >= BATCH_SIZE) flush()
      else schedule(time + FLUSH_DELAY_MS)
    } finally {
      busy = false
    }
  }

  const flush = () => {
    cancelTimer?.()
    cancelTimer = null
    timerDeadline = Infinity

    const time = options.now()
    const events = ready.splice(0)
    let nextDeadline = Infinity

    for (const [id, current] of windows) {
      const end = current.start + DEDUPE_WINDOW_MS
      const expired = time >= end
      if (current.open && (!current.held || expired)) {
        events.push(current.open)
        current.open = null
        current.held = false
      }
      if (expired) windows.delete(id)
      else if (current.open) nextDeadline = Math.min(nextDeadline, end)
    }
    queued = 0

    if (events.length > 0 || dropped > 0) {
      const batch: FrontendEventBatch = { events, dropped }
      dropped = 0
      Promise.resolve()
        .then(() => {
          busy = true
          try {
            return options.send(batch)
          } finally {
            busy = false
          }
        })
        // Not retried and not logged: logging here would report itself.
        .catch(() => {
          dropped += events.length
        })
    }

    if (nextDeadline < Infinity) schedule(nextDeadline)
  }

  const dispose = () => {
    flush()
    cancelTimer?.()
    cancelTimer = null
    timerDeadline = Infinity
  }

  return { capture, flush, dispose }
}
