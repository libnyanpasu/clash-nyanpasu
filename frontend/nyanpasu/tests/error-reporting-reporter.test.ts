import { expect, test } from 'vitest'
import { fromConsole } from '@/services/error-reporting/normalize'
import { createReporter } from '@/services/error-reporting/reporter'
import type { FrontendEventBatch } from '@nyanpasu/rpc/types'

/** A manual clock and timer queue; `advance` runs due timers in order. */
function harness(send?: (batch: FrontendEventBatch) => Promise<void>) {
  let time = 1_000_000
  let timers: { at: number; callback: () => void; id: number }[] = []
  let nextId = 0
  const batches: FrontendEventBatch[] = []

  const reporter = createReporter({
    send:
      send ??
      (async (batch) => {
        batches.push(batch)
      }),
    now: () => time,
    route: () => '/main/dashboard',
    schedule: (callback, ms) => {
      const id = nextId++
      timers.push({ at: time + ms, callback, id })
      return () => {
        timers = timers.filter((timer) => timer.id !== id)
      }
    },
  })

  const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

  const advance = async (ms: number) => {
    const target = time + ms
    for (;;) {
      const due = timers
        .filter((timer) => timer.at <= target)
        .sort((a, b) => a.at - b.at)[0]
      if (!due) break
      timers = timers.filter((timer) => timer !== due)
      time = due.at
      due.callback()
    }
    time = target
    await settle()
  }

  return {
    reporter,
    batches,
    advance,
    settle,
    pendingTimers: () => timers.length,
  }
}

const error = (message: string) =>
  fromConsole('error', [message, new Error(message)])

test('repeats within one batch are merged into one counted event', async () => {
  const { reporter, batches, advance } = harness()

  for (let i = 0; i < 100; i++) reporter.capture(error('boom'))
  await advance(1_000)

  expect(batches).toHaveLength(1)
  expect(batches[0].dropped).toBe(0)
  expect(batches[0].events).toHaveLength(1)
  expect(batches[0].events[0]).toMatchObject({
    message: 'boom Error: boom',
    count: 100,
    route: '/main/dashboard',
    first_seen_ms: 1_000_000,
  })
})

test('a repeat after the first send is held until its window ends', async () => {
  const { reporter, batches, advance } = harness()

  reporter.capture(error('boom'))
  await advance(1_000)
  reporter.capture(error('boom'))
  reporter.capture(error('boom'))
  await advance(30_000)
  expect(batches).toHaveLength(1)

  await advance(30_000)
  expect(batches).toHaveLength(2)
  expect(batches[1].events[0].count).toBe(2)

  // The window ended: the next occurrence is reported promptly again.
  reporter.capture(error('boom'))
  await advance(1_000)
  expect(batches).toHaveLength(3)
  expect(batches[2].events[0].count).toBe(1)
})

test('a full batch is sent at once', async () => {
  const { reporter, batches, settle } = harness()

  for (let i = 0; i < 16; i++)
    reporter.capture(error(`distinct ${'x'.repeat(i)}`))
  await settle()

  expect(batches).toHaveLength(1)
  expect(batches[0].events).toHaveLength(16)
})

test('distinct events beyond the rate limit are counted as dropped', async () => {
  const { reporter, batches, advance } = harness()

  for (let i = 0; i < 200; i++) reporter.capture(error(`e${'x'.repeat(i)}`))
  await advance(1_000)

  const sent = batches.flatMap((batch) => batch.events)
  const dropped = batches.reduce((sum, batch) => sum + batch.dropped, 0)
  expect(sent).toHaveLength(60)
  expect(dropped).toBe(140)

  await advance(60_000)
  reporter.capture(error('after the window'))
  await advance(1_000)
  expect(batches.at(-1)?.events[0].message).toContain('after the window')
})

test('a failed send is not retried and is reported as dropped', async () => {
  let fail = true
  const batches: FrontendEventBatch[] = []
  const { reporter, advance } = harness(async (batch) => {
    if (fail) throw new Error('offline')
    batches.push(batch)
  })

  reporter.capture(error('a'))
  reporter.capture(error('b'))
  await advance(1_000)
  expect(batches).toHaveLength(0)

  fail = false
  reporter.capture(error('c'))
  await advance(1_000)
  expect(batches).toHaveLength(1)
  expect(batches[0].events.map((event) => event.message)).toEqual([
    'c Error: c',
  ])
  expect(batches[0].dropped).toBe(2)
})

test('logging done while sending is not captured', async () => {
  const batches: FrontendEventBatch[] = []
  let reporterRef: ReturnType<typeof createReporter> | null = null
  const { reporter, advance } = harness(async (batch) => {
    reporterRef?.capture(error('from the transport'))
    batches.push(batch)
  })
  reporterRef = reporter

  reporter.capture(error('original'))
  await advance(5_000)

  expect(batches).toHaveLength(1)
  expect(batches[0].events.map((event) => event.message)).toEqual([
    'original Error: original',
  ])
})

test('dispose sends what is queued and stops the timer', async () => {
  const { reporter, batches, settle, pendingTimers } = harness()

  reporter.capture(error('last words'))
  reporter.dispose()
  await settle()

  expect(batches).toHaveLength(1)
  expect(pendingTimers()).toBe(0)
})
