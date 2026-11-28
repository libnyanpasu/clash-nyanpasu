import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import ManagedGuardFieldSelector from '../src/pages/(main)/main/settings/clash/_modules/managed-guard-field-selector'

const backend = vi.hoisted(() => ({
  settings: vi.fn(),
  access: vi.fn(),
  message: vi.fn(),
}))
vi.mock('@nyanpasu/query', () => ({
  useClashSettings: () => ({
    query: {
      data: {
        overrides: {
          ipv6: { kind: 'unmanaged' },
          'allow-lan': { kind: 'managed', value: false },
        },
      },
    },
  }),
  useClashConfig: () => ({ upsert: { mutateAsync: backend.settings } }),
  useClashAccess: () => ({ upsert: { mutateAsync: backend.access } }),
}))
vi.mock('@/utils/notification', () => ({ message: backend.message }))

test('shows that ipv6 follows the profile and forces it through the settings request', async ({
  onTestFinished,
}) => {
  backend.settings.mockResolvedValue(undefined)
  const view = await render(<ManagedGuardFieldSelector field="ipv6" />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const trigger = view.getByRole('button', {
    name: new RegExp(m.settings_clash_settings_ipv6_label()),
  })
  await expect
    .poll(() => trigger.element().textContent)
    .toContain(m.settings_clash_settings_managed_guard_field_unmanaged())

  await trigger.click()
  await view
    .getByRole('menuitemcheckbox', {
      name: m.settings_clash_settings_managed_guard_field_enabled(),
    })
    .click()

  await expect
    .poll(() => backend.settings.mock.lastCall)
    .toEqual([{ ipv6: { kind: 'managed', value: true } }])
  expect(backend.access).not.toHaveBeenCalled()
})

test('hands allow-lan to the profile through the access request', async ({
  onTestFinished,
}) => {
  backend.access.mockResolvedValue(undefined)
  const view = await render(<ManagedGuardFieldSelector field="allow-lan" />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const trigger = view.getByRole('button', {
    name: new RegExp(m.settings_clash_settings_allow_lan_label()),
  })
  await expect
    .poll(() => trigger.element().textContent)
    .toContain(m.settings_clash_settings_managed_guard_field_disabled())

  await trigger.click()
  await view
    .getByRole('menuitemcheckbox', {
      name: m.settings_clash_settings_managed_guard_field_unmanaged(),
    })
    .click()

  await expect
    .poll(() => backend.access.mock.lastCall)
    .toEqual([{ 'allow-lan': { kind: 'unmanaged' } }])
  expect(backend.settings).not.toHaveBeenCalled()
})
