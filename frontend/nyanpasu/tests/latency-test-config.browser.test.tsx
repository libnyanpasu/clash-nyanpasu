import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import LatencyTestConfig from '../src/pages/(main)/main/settings/clash/_modules/latency-test-config'

const backend = vi.hoisted(() => ({
  values: {} as Record<string, unknown>,
  upsert: vi.fn(),
}))
vi.mock('@nyanpasu/query', async (importActual) => ({
  ...(await importActual<typeof import('@nyanpasu/query')>()),
  useSetting: (key: string) => {
    const [value, setValue] = useState(backend.values[key])
    return {
      value,
      isPending: false,
      upsert: async (next: unknown) => {
        await backend.upsert(key, next)
        setValue(next)
      },
    }
  },
}))

vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

async function setup(onTestFinished: (fn: () => Promise<void>) => void) {
  backend.values = {
    default_latency_test: 'http://www.gstatic.com/generate_204',
    default_latency_timeout_ms: 5000,
  }
  backend.upsert.mockResolvedValue(undefined)
  const view = await render(<LatencyTestConfig />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  return {
    view,
    url: view.getByRole('textbox', {
      name: m.settings_clash_latency_test_url_label(),
    }),
    timeout: view.getByRole('textbox', {
      name: m.settings_clash_latency_test_timeout_label(),
    }),
    apply: () => view.getByRole('button', { name: m.common_apply() }),
  }
}

test('shows the saved URL and the timeout in seconds', async ({
  onTestFinished,
}) => {
  const { url, timeout, view } = await setup(onTestFinished)
  await expect.element(url).toHaveValue('http://www.gstatic.com/generate_204')
  await expect.element(timeout).toHaveValue('5')
  await expect
    .element(view.getByText(m.settings_clash_latency_test_url_description()))
    .toBeInTheDocument()
})

test('saves a changed URL only', async ({ onTestFinished }) => {
  const { url, apply } = await setup(onTestFinished)
  await url.fill('https://cp.cloudflare.com/generate_204')
  await apply().click()
  await expect
    .poll(() => backend.upsert.mock.calls)
    .toEqual([
      ['default_latency_test', 'https://cp.cloudflare.com/generate_204'],
    ])
})

test('saves the timeout in milliseconds', async ({ onTestFinished }) => {
  const { timeout, apply } = await setup(onTestFinished)
  await timeout.fill('8')
  await apply().click()
  await expect
    .poll(() => backend.upsert.mock.calls)
    .toEqual([['default_latency_timeout_ms', 8000]])
})

for (const value of ['0', '31']) {
  test(`rejects timeout ${value} without saving`, async ({
    onTestFinished,
  }) => {
    const { timeout, apply, view } = await setup(onTestFinished)
    await timeout.fill(value)
    await apply().click()
    await expect
      .element(view.getByText(m.settings_clash_latency_test_invalid_timeout()))
      .toBeInTheDocument()
    expect(backend.upsert).not.toHaveBeenCalled()
  })
}

for (const value of ['ftp://x', 'https://[', 'https://example.com:99999']) {
  test(`rejects the URL ${value} without saving`, async ({
    onTestFinished,
  }) => {
    const { url, apply, view } = await setup(onTestFinished)
    await url.fill(value)
    await apply().click()
    const error = view.getByText(m.settings_clash_latency_test_invalid_url())
    await expect.element(error).toBeInTheDocument()
    await expect.element(url).toHaveAttribute('aria-invalid', 'true')
    await expect
      .element(url)
      .toHaveAttribute('aria-describedby', error.element().id)
    expect(backend.upsert).not.toHaveBeenCalled()
  })
}

test('an empty URL restores the default', async ({ onTestFinished }) => {
  const { url, apply } = await setup(onTestFinished)
  await url.fill('https://cp.cloudflare.com/generate_204')
  await apply().click()
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(1)
  backend.upsert.mockClear()

  await url.clear()
  await apply().click()
  await expect
    .poll(() => backend.upsert.mock.calls)
    .toEqual([['default_latency_test', 'http://www.gstatic.com/generate_204']])
  await expect.element(url).toHaveValue('http://www.gstatic.com/generate_204')
})

test('an invalid timeout is described by its error', async ({
  onTestFinished,
}) => {
  const { timeout, apply, view } = await setup(onTestFinished)
  await timeout.fill('31')
  await apply().click()
  const error = view.getByText(m.settings_clash_latency_test_invalid_timeout())
  await expect.element(error).toBeInTheDocument()
  await expect.element(timeout).toHaveAttribute('aria-invalid', 'true')
  await expect
    .element(timeout)
    .toHaveAttribute('aria-describedby', error.element().id)
})

test('reset restores the saved values', async ({ onTestFinished }) => {
  const { url, timeout, view } = await setup(onTestFinished)
  await url.fill('https://example.com')
  await timeout.fill('9')
  await view.getByRole('button', { name: m.common_reset() }).click()
  await expect.element(url).toHaveValue('http://www.gstatic.com/generate_204')
  await expect.element(timeout).toHaveValue('5')
  expect(backend.upsert).not.toHaveBeenCalled()
})
