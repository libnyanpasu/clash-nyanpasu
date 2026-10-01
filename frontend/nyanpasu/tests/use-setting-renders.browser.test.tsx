import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { useSetting } from '@nyanpasu/interface'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'

test('a setting consumer does not re-render when a refetch finds nothing new', async ({
  onTestFinished,
}) => {
  mockIPC((wireCommand, args) => {
    expect(wireCommand).toBe('call_rpc')
    const { method } = args as { method: string }
    expect(method).toBe('get_app_config')
    return { theme_mode: 'dark', always_on_top: false }
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  let renders = 0
  function Probe() {
    const { value } = useSetting('theme_mode')
    renders += 1
    return <span>{value}</span>
  }
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
    clearMocks()
  })
  root.render(
    <QueryClientProvider client={queries}>
      <Probe />
    </QueryClientProvider>,
  )
  await expect.poll(() => container.textContent).toBe('dark')

  const before = renders
  await queries.refetchQueries()
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(renders - before).toBe(0)
})
