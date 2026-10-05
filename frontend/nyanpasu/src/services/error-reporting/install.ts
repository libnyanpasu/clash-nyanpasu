import type { RootOptions } from 'react-dom/client'
import {
  fromConsole,
  fromReact,
  fromRejection,
  fromWindowError,
} from './normalize'
import type { Reporter } from './reporter'

export type ErrorReporting = {
  /** Pass to `createRoot`; replaces React's default error logging. */
  rootOptions: Pick<
    RootOptions,
    'onUncaughtError' | 'onCaughtError' | 'onRecoverableError'
  >
  uninstall: () => void
}

/** The parts of `window` the reporting hooks use. */
export type ReportingTarget = Pick<
  Window,
  'addEventListener' | 'removeEventListener'
> & {
  document: Pick<
    Document,
    'visibilityState' | 'addEventListener' | 'removeEventListener'
  >
}

/**
 * Forwards console warnings and errors, uncaught errors, unhandled
 * rejections and React errors to `reporter`. The original console output
 * is kept.
 */
export function installErrorReporting(
  reporter: Reporter,
  target: ReportingTarget = window,
  console: Pick<Console, 'warn' | 'error'> = globalThis.console,
): ErrorReporting {
  const originalWarn = console.warn
  const originalError = console.error

  const warn = (...args: unknown[]) => {
    originalWarn.apply(console, args)
    reporter.capture(fromConsole('warning', args))
  }
  const error = (...args: unknown[]) => {
    originalError.apply(console, args)
    reporter.capture(fromConsole('error', args))
  }
  console.warn = warn
  console.error = error

  // Capture phase: resource load errors do not bubble to the window.
  const onError = (event: Event) => reporter.capture(fromWindowError(event))
  const onRejection = (event: PromiseRejectionEvent) =>
    reporter.capture(fromRejection(event.reason))
  const onHidden = () => {
    if (target.document.visibilityState === 'hidden') reporter.flush()
  }
  const onPageHide = () => reporter.flush()

  target.addEventListener('error', onError, true)
  target.addEventListener('unhandledrejection', onRejection)
  target.document.addEventListener('visibilitychange', onHidden)
  target.addEventListener('pagehide', onPageHide)

  // React logs these itself by default; keep that output without reporting it twice.
  const rootOptions: ErrorReporting['rootOptions'] = {
    onUncaughtError: (thrown, info) => {
      originalError.call(console, thrown)
      reporter.capture(fromReact('react_uncaught', thrown, info.componentStack))
    },
    onCaughtError: (thrown, info) => {
      originalError.call(console, thrown)
      reporter.capture(fromReact('react_caught', thrown, info.componentStack))
    },
    onRecoverableError: (thrown, info) => {
      originalWarn.call(console, thrown)
      reporter.capture(
        fromReact('react_recoverable', thrown, info.componentStack),
      )
    },
  }

  const uninstall = () => {
    if (console.warn === warn) console.warn = originalWarn
    if (console.error === error) console.error = originalError
    target.removeEventListener('error', onError, true)
    target.removeEventListener('unhandledrejection', onRejection)
    target.document.removeEventListener('visibilitychange', onHidden)
    target.removeEventListener('pagehide', onPageHide)
    reporter.dispose()
  }

  return { rootOptions, uninstall }
}
