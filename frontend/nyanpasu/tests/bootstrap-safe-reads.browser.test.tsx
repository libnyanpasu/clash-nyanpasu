import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { useProxyMode } from '@nyanpasu/query'
import { createIconTransport, createRpcClient } from '@nyanpasu/rpc'
import { QueryClient } from '@tanstack/react-query'
import { LanguageProvider } from '../src/components/providers/language-provider'
import { ExperimentalThemeProvider } from '../src/components/providers/theme-provider'
import useCurrentCoreIcon from '../src/hooks/use-current-core-icon'
import { TestQueryProvider } from './query-provider'

// Reads that need `sensitive_read`: the full configs carry the PAC and latency
// URLs, the web UI templates and the controller secret.
const SENSITIVE_READS = [
  'get_app_config',
  'get_clash_config',
  'clash_api_get_configs',
  'get_clash_info',
]

function Bootstrap() {
  useCurrentCoreIcon()
  useProxyMode()
  return null
}

test('the theme, language, core and proxy-mode readers use only safe queries', async ({
  onTestFinished,
}) => {
  const methods: string[] = []
  const invoke = async (method: string) => {
    methods.push(method)
    switch (method) {
      case 'get_app_settings':
        return {
          theme_mode: 'dark',
          theme_color: '#ff0000',
          language: 'en',
          core: 'mihomo',
          pac_configured: true,
          latency_test_configured: true,
        }
      case 'clash_api_get_settings':
        return { mode: 'rule', 'external-controller': '127.0.0.1:9090' }
      default:
        // Any other read is not under test; a sensitive one is caught below.
        return null
    }
  }
  const rpc = createRpcClient({
    icons: createIconTransport(false),
    commands: { invoke: async (m: string) => invoke(m) as never },
    events: {
      listen: async () => () => {},
      once: async () => () => {},
      emit: async () => {},
      listenResync: () => () => {},
      dispose: () => {},
    },
  })
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
  })

  root.render(
    <TestQueryProvider client={queries} rpc={rpc}>
      <LanguageProvider>
        <ExperimentalThemeProvider>
          <Bootstrap />
        </ExperimentalThemeProvider>
      </LanguageProvider>
    </TestQueryProvider>,
  )

  await expect.poll(() => methods).toContain('get_app_settings')
  await expect.poll(() => methods).toContain('clash_api_get_settings')
  await new Promise((resolve) => setTimeout(resolve, 100))

  for (const sensitive of SENSITIVE_READS) {
    expect(methods).not.toContain(sensitive)
  }
})
