import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import ProxyGuardConfig from '../src/pages/(main)/main/settings/system/proxy/_modules/proxy-guard-config'

const backend = vi.hoisted(() => ({
  value: 10,
  upsert: vi.fn(),
}))

vi.mock('@nyanpasu/query', async (importActual) => ({
  ...(await importActual<typeof import('@nyanpasu/query')>()),
  useSystemSetting: () => {
    const [value, setValue] = useState(backend.value)

    return {
      value,
      isPending: false,
      upsert: async (next: number) => {
        await backend.upsert(next)
        setValue(next)
      },
    }
  },
}))

vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

async function setup(onTestFinished: (fn: () => Promise<void>) => void) {
  backend.value = 10
  backend.upsert.mockResolvedValue(undefined)

  const view = await render(<ProxyGuardConfig />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const open = () =>
    view
      .getByRole('button', {
        name: m.settings_system_proxy_proxy_guard_interval_label(),
        exact: false,
      })
      .click()

  await open()

  return {
    view,
    open,
    dialog: view.getByRole('dialog'),
    interval: view.getByRole('textbox', {
      name: m.settings_system_proxy_proxy_guard_interval_label(),
    }),
    apply: view.getByRole('button', { name: m.common_apply() }),
    reset: view.getByRole('button', { name: m.common_reset() }),
  }
}

test('opens the dialog with the saved interval', async ({ onTestFinished }) => {
  const { interval } = await setup(onTestFinished)

  await expect.element(interval).toHaveValue('10')
})

test('saves the interval and closes the dialog', async ({ onTestFinished }) => {
  const { interval, apply, dialog } = await setup(onTestFinished)

  await interval.fill('15')
  await apply.click()

  await expect.poll(() => backend.upsert.mock.calls).toEqual([[15]])
  await expect.element(dialog).not.toBeInTheDocument()
})

test('reset restores the saved interval', async ({ onTestFinished }) => {
  const { interval, reset } = await setup(onTestFinished)

  await interval.fill('15')
  await reset.click()

  await expect.element(interval).toHaveValue('10')
  expect(backend.upsert).not.toHaveBeenCalled()
})
