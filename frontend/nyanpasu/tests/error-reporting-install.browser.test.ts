import { expect, test, vi } from 'vitest'
import {
  installErrorReporting,
  type ReportingTarget,
} from '@/services/error-reporting/install'
import {
  fromWindowError,
  type EventDraft,
} from '@/services/error-reporting/normalize'
import type { Reporter } from '@/services/error-reporting/reporter'

/**
 * A stand-in window: dispatching real error events on the test window would
 * be reported by the test runner itself.
 */
function fakeTarget() {
  const events = new EventTarget()
  const document = Object.assign(new EventTarget(), {
    visibilityState: 'visible' as DocumentVisibilityState,
  })
  const console = { warn: vi.fn(), error: vi.fn() }
  const original = { ...console }
  // Narrow boundary assertion: the hooks only use these members.
  const target = Object.assign(events, {
    document,
  }) as unknown as ReportingTarget
  return { target, console, original, document }
}

function fakeReporter() {
  const drafts: EventDraft[] = []
  const reporter: Reporter = {
    capture: (draft) => drafts.push(draft),
    flush: vi.fn(),
    dispose: vi.fn(),
  }
  return { reporter, drafts }
}

test('console calls keep their output and are captured', () => {
  const { target, console, original } = fakeTarget()
  const { reporter, drafts } = fakeReporter()
  installErrorReporting(reporter, target, console)

  console.warn('careful', 1)
  console.error(new Error('broken'))

  expect(original.warn).toHaveBeenCalledWith('careful', 1)
  expect(original.error).toHaveBeenCalledOnce()
  expect(
    drafts.map(({ kind, level, message }) => [kind, level, message]),
  ).toEqual([
    ['console', 'warning', 'careful 1'],
    ['console', 'error', 'Error: broken'],
  ])
})

test('uncaught errors and rejections are captured', () => {
  const { target, console } = fakeTarget()
  const { reporter, drafts } = fakeReporter()
  installErrorReporting(reporter, target, console)

  const thrown = new TypeError('in a callback')
  ;(target as unknown as EventTarget).dispatchEvent(
    new ErrorEvent('error', { error: thrown, message: thrown.message }),
  )
  const promise = Promise.reject(new Error('async'))
  promise.catch(() => {})
  ;(target as unknown as EventTarget).dispatchEvent(
    new PromiseRejectionEvent('unhandledrejection', {
      promise,
      reason: new Error('async'),
    }),
  )

  expect(drafts).toMatchObject([
    {
      kind: 'uncaught_error',
      error_name: 'TypeError',
      message: 'in a callback',
    },
    { kind: 'unhandled_rejection', message: 'async' },
  ])
})

test('an error event without an error object keeps its location', () => {
  expect(
    fromWindowError(
      new ErrorEvent('error', {
        message: 'Script error.',
        filename: 'http://localhost/app.js?token=x',
        lineno: 3,
        colno: 9,
      }),
    ),
  ).toMatchObject({
    message: 'Script error.',
    stack: 'at http://localhost/app.js:3:9',
  })
})

test('a failed resource load names the element and its path', () => {
  const image = document.createElement('img')
  image.src = 'http://localhost/missing.png?sig=secret'
  let draft: EventDraft | null = null
  image.addEventListener('error', (event) => {
    draft = fromWindowError(event)
  })

  image.dispatchEvent(new Event('error'))

  expect(draft).toMatchObject({
    kind: 'uncaught_error',
    message: 'failed to load <img> http://localhost/missing.png',
  })
})

test('React root errors keep the default output and are reported once', () => {
  const { target, console, original } = fakeTarget()
  const { reporter, drafts } = fakeReporter()
  const { rootOptions } = installErrorReporting(reporter, target, console)
  const info = { componentStack: '\n    at Page' }

  rootOptions.onCaughtError?.(new Error('caught'), info)
  rootOptions.onUncaughtError?.(new Error('uncaught'), info)
  rootOptions.onRecoverableError?.(new Error('recovered'), info)

  expect(original.error).toHaveBeenCalledTimes(2)
  expect(original.warn).toHaveBeenCalledOnce()
  expect(console.error).not.toBe(original.error)
  expect(drafts.map(({ kind, level }) => [kind, level])).toEqual([
    ['react_caught', 'error'],
    ['react_uncaught', 'error'],
    ['react_recoverable', 'warning'],
  ])
  expect(drafts[0].component_stack).toBe('\n    at Page')
})

test('hiding the page flushes and uninstall restores everything', () => {
  const { target, console, original, document } = fakeTarget()
  const { reporter, drafts } = fakeReporter()
  const { uninstall } = installErrorReporting(reporter, target, console)

  document.visibilityState = 'hidden'
  document.dispatchEvent(new Event('visibilitychange'))
  ;(target as unknown as EventTarget).dispatchEvent(new Event('pagehide'))
  expect(reporter.flush).toHaveBeenCalledTimes(2)

  uninstall()
  expect(console.warn).toBe(original.warn)
  expect(console.error).toBe(original.error)
  expect(reporter.dispose).toHaveBeenCalledOnce()

  console.error('after')
  ;(target as unknown as EventTarget).dispatchEvent(
    new ErrorEvent('error', { message: 'after' }),
  )
  expect(drafts).toEqual([])

  // Reinstalling (as after a hot update) wraps the console exactly once.
  installErrorReporting(reporter, target, console).uninstall()
  installErrorReporting(reporter, target, console)
  console.error('once')
  expect(original.error).toHaveBeenCalledTimes(2)
  expect(drafts).toHaveLength(1)
})
