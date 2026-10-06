import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import BreakWhenProxyChangeSelector from '../src/pages/(main)/main/settings/nyanpasu/_modules/break-when-proxy-change-selector'

const backend = vi.hoisted(() => ({
  upsert: vi.fn(),
  message: vi.fn(),
}))
vi.mock('@nyanpasu/query', () => ({
  useClashSetting: () => ({
    value: {
      on_proxy_change: 'all',
      on_profile_change: false,
      on_mode_change: true,
    },
    isPending: false,
    upsert: backend.upsert,
  }),
}))
vi.mock('@/utils/notification', () => ({ message: backend.message }))

test('shows the current mode and saves the selected one', async ({
  onTestFinished,
}) => {
  backend.upsert.mockResolvedValue(undefined)
  const view = await render(<BreakWhenProxyChangeSelector />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await expect
    .element(
      view.getByRole('radiogroup', {
        name: m.settings_nyanpasu_enhance_break_when_proxy_change_label(),
      }),
    )
    .toBeInTheDocument()

  await expect
    .element(
      view.getByRole('radio', {
        name: m.settings_nyanpasu_enhance_break_when_proxy_change_all(),
      }),
    )
    .toBeChecked()

  await view
    .getByRole('radio', {
      name: m.settings_nyanpasu_enhance_break_when_proxy_change_group(),
    })
    .click()

  await expect
    .poll(() => backend.upsert.mock.lastCall)
    .toEqual([{ on_proxy_change: 'proxy_group' }])
})

test('shows an error when saving fails', async ({ onTestFinished }) => {
  const error = new Error('save failed')
  backend.upsert.mockRejectedValue(error)
  const view = await render(<BreakWhenProxyChangeSelector />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('radio', {
      name: m.settings_nyanpasu_enhance_break_when_proxy_change_off(),
    })
    .click()

  await expect.poll(() => backend.message.mock.calls.length).toBe(1)
  expect(backend.message.mock.lastCall?.[1]).toMatchObject({
    kind: 'error',
    error,
  })
})
