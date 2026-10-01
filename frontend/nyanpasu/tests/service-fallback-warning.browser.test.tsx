import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import ServiceFallbackWarning from '@/components/settings/service-fallback-warning'
import { TooltipProvider } from '@/components/ui/tooltip'
import { MutationProvider } from '@interface/provider/mutation-provider'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'

// The interface picks the Tauri event transport at import time.
vi.hoisted(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} })
})

const coreStatus = (host: 'local' | 'service') => ({
  controller: null,
  host,
  connectivity: { kind: 'connected' },
  generation: 1,
  state: { Running: { epoch: 1, pid: 7 } },
  state_changed_at: 0,
  revision: null,
  healthy: true,
})

// #5443: service mode on while the core runs locally is flagged next to the
// switch, and the core status event alone clears it once the service runs it.
test('the warning follows the host the core runs on', async ({
  onTestFinished,
}) => {
  let host: 'local' | 'service' = 'local'
  mockIPC(
    (wireCommand, args) => {
      expect(wireCommand).toBe('call_rpc')
      const { method } = args as { method: string }
      switch (method) {
        case 'get_app_config':
          return { enable_service_mode: true }
        case 'get_core_status':
          return coreStatus(host)
        case 'status_service':
          return {
            name: 'nyanpasu-service',
            version: '',
            status: 'not_installed',
            server: null,
            compat: { kind: 'unknown' },
            phase: 'not_installed',
            restart_attempts: 0,
          }
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
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
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
  await emit('core-status-changed-event', coreStatus(host))
  await expect.poll(warning).toBeNull()
})
