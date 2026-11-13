import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import ServiceFallbackWarning from '@/components/settings/service-fallback-warning'
import { MutationProvider } from '@nyanpasu/query/provider'
import { QueryClient } from '@tanstack/react-query'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Client construction selects the desktop event adapter for this test.
vi.hoisted(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {}, isTauri: true })
})

const coreStatus = (host: 'local' | 'service') => ({
  controller: null,
  host,
  connectivity: { kind: 'connected' },
  generation: 1,
  state: { Running: { epoch: 1 } },
  state_changed_at: 0,
  revision: null,
  healthy: true,
})

// #5443: service mode on while the core runs locally is flagged next to the
// switch, and the core status event alone clears it once the service runs it.
// The flag reads only the safe summaries; the full status queries carry the
// daemon's data directories and the core's diagnostic text.
test('the warning follows the host the core runs on', async ({
  onTestFinished,
}) => {
  let host: 'local' | 'service' = 'local'
  mockIPC(
    (wireCommand, args) => {
      expect(wireCommand).toBe('call_rpc')
      const { method } = args as { method: string }
      switch (method) {
        case 'get_app_settings':
          return { enable_service_mode: true }
        case 'get_core_status_summary':
          return coreStatus(host)
        case 'get_service_status_summary':
          return {
            name: 'nyanpasu-service',
            version: '',
            status: 'not_installed',
            server_reported: false,
            compat: { kind: 'unknown' },
            phase: 'not_installed',
            restart_attempts: 0,
          }
        case 'get_core_status':
        case 'status_service':
          throw new Error(`Protected query read by an ordinary view: ${method}`)
        default:
          return null
      }
    },
    { shouldMockEvents: true },
  )
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(async () => {
    root.unmount()
    container.remove()
    queries.clear()
    // Resolve provider subscription cleanup before clearing the native adapter.
    await Promise.resolve()
    clearMocks()
  })
  const warning = () =>
    container.querySelector('[data-slot="service-fallback-warning"]')

  root.render(
    <QueryClientProvider client={queries}>
      <MutationProvider>
        <TooltipProvider>
          <ServiceFallbackWarning />
        </TooltipProvider>
      </MutationProvider>
    </QueryClientProvider>,
  )

  await expect.poll(warning).toBeTruthy()
  expect(warning()?.getAttribute('aria-label')).toContain('not installed')

  host = 'service'
  await emit('core-status-summary-changed-event', coreStatus(host))
  await expect.poll(warning).toBeNull()
})
