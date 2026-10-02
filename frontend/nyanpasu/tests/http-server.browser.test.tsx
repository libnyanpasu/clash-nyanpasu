import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import HttpServer from '../src/pages/(main)/main/settings/debug/_modules/http-server'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

const backend = vi.hoisted(() => ({
  desktop: true,
  status: vi.fn(),
  set: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => backend.desktop,
}))
vi.mock('@nyanpasu/utils', () => ({
  cn: (...values: unknown[]) => values.filter(Boolean).join(' '),
}))
vi.mock('@/services/rpc', () => ({
  rpc: { getDebugHttpStatus: backend.status, setDebugHttpEnabled: backend.set },
}))

async function setup(
  onTestFinished: TestContext['onTestFinished'],
  desktop = true,
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  backend.desktop = desktop
  backend.status.mockResolvedValue({
    status: 'ok',
    data: { enabled: false, url: null },
  })
  const view = await render(
    <QueryClientProvider client={client}>
      <HttpServer />
    </QueryClientProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    vi.clearAllMocks()
  })
  const toggle = view.getByRole('switch', { name: 'Axum HTTP server' })
  if (desktop) await expect.element(toggle).toBeEnabled()
  else await expect.element(toggle).toBeDisabled()
  return { view, toggle }
}

test('switch updates only after server acknowledgement and shows its URL', async ({
  onTestFinished,
}) => {
  const { view, toggle } = await setup(onTestFinished)
  let acknowledge!: (value: unknown) => void
  backend.set.mockImplementation(
    () =>
      new Promise((resolve) => {
        acknowledge = resolve
      }),
  )
  await toggle.click()
  await expect.element(toggle).toBeDisabled()
  await expect.element(toggle).not.toBeChecked()
  acknowledge({
    status: 'ok',
    data: { enabled: true, url: 'http://127.0.0.1:12345' },
  })
  await expect.element(toggle).toBeChecked()
  await expect
    .element(view.getByRole('link'))
    .toHaveAttribute('href', 'http://127.0.0.1:12345')
  backend.set.mockResolvedValue({
    status: 'ok',
    data: { enabled: false, url: null },
  })
  await toggle.click()
  await expect.element(toggle).not.toBeChecked()
  expect(backend.set).toHaveBeenNthCalledWith(1, true)
  expect(backend.set).toHaveBeenNthCalledWith(2, false)
})

test('failed start keeps the switch off and reports the error', async ({
  onTestFinished,
}) => {
  const { view, toggle } = await setup(onTestFinished)
  backend.set.mockResolvedValue({
    status: 'error',
    error: { message: 'bind failed' },
  })
  await toggle.click()
  await expect.element(view.getByRole('alert')).toHaveTextContent('bind failed')
  await expect.element(toggle).not.toBeChecked()
})

test('browser can inspect status but cannot stop its own transport', async ({
  onTestFinished,
}) => {
  const { toggle } = await setup(onTestFinished, false)
  await expect.element(toggle).toBeDisabled()
  expect(backend.set).not.toHaveBeenCalled()
})
