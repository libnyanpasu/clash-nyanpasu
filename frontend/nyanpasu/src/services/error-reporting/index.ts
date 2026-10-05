import { rpc } from '@/services/rpc'
import { unwrapResult } from '@nyanpasu/rpc'
import { installErrorReporting } from './install'
import { createReporter } from './reporter'

// This webview's reporter; frontend events go to the application log.
const reporter = createReporter({
  send: async (batch) => {
    unwrapResult(await rpc.reportFrontendEvents(batch))
  },
  now: Date.now,
  route: () => window.location.pathname,
  schedule: (callback, ms) => {
    const id = setTimeout(callback, ms)
    return () => clearTimeout(id)
  },
})

const reporting = installErrorReporting(reporter)

export const reactRootErrorOptions = reporting.rootOptions

if (import.meta.hot) {
  import.meta.hot.dispose(() => reporting.uninstall())
}
