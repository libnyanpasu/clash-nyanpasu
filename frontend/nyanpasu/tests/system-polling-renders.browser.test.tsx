import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { useSystemAccentColor, useSystemProxy } from '@nyanpasu/query'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

test('polling an unchanged system proxy and accent color does not re-render', async ({
  onTestFinished,
}) => {
  const polls = { proxy: 0, accent: 0 }
  mockIPC((wireCommand, args) => {
    expect(wireCommand).toBe('call_rpc')
    const { method } = args as { method: string }
    switch (method) {
      case 'get_sys_proxy':
        polls.proxy += 1
        return { enable: true, server: '127.0.0.1:7890', bypass: 'localhost' }
      case 'get_system_accent_color':
        polls.accent += 1
        return '#1867c0'
      case 'get_app_config':
        return { enable_system_proxy: true }
      default:
        throw new Error(`Unexpected command: ${method}`)
    }
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  let renders = 0
  function Probe() {
    const { data } = useSystemProxy()
    const { systemAccentColor } = useSystemAccentColor()
    renders += 1
    return (
      <span>
        {data?.server} {systemAccentColor}
      </span>
    )
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
  await expect.poll(() => container.textContent).toBe('127.0.0.1:7890 #1867c0')
  await new Promise((resolve) => setTimeout(resolve, 200))

  const before = renders
  await new Promise((resolve) => setTimeout(resolve, 5500))
  expect(polls.proxy).toBeGreaterThan(1)
  expect(polls.accent).toBeGreaterThan(1)
  expect(renders - before).toBe(0)
})
