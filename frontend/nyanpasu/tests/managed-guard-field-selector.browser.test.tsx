import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import type { ManageableField } from '@nyanpasu/rpc/types'
import ManagedGuardFieldSelector from '../src/pages/(main)/main/settings/clash/_modules/managed-guard-field-selector'

const backend = vi.hoisted(() => ({
  value: { kind: 'unmanaged' } as ManageableField<boolean>,
  patchSettings: vi.fn(),
  patchOverrides: vi.fn(),
}))

vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

vi.mock('@nyanpasu/query', () => ({
  useClashSettings: () => ({
    query: {
      data: {
        enable_tun_mode: backend.value,
        tun_stack: 'system',
        overrides: {
          'unified-delay': backend.value,
          'tcp-concurrent': backend.value,
        },
      },
    },
    upsert: { mutateAsync: backend.patchSettings },
  }),
  useClashConfig: () => ({ upsert: { mutateAsync: backend.patchOverrides } }),
}))

test.for(['enable_tun_mode', 'unified-delay', 'tcp-concurrent'] as const)(
  'the shared selector writes %s through its existing configuration entrypoint',
  async (field, { onTestFinished }) => {
    backend.value = { kind: 'unmanaged' }
    backend.patchSettings.mockResolvedValue(undefined)
    backend.patchOverrides.mockResolvedValue(undefined)
    const view = await render(<ManagedGuardFieldSelector field={field} />)
    onTestFinished(async () => {
      await view.unmount()
      vi.clearAllMocks()
    })
    await expect
      .poll(() => view.getByRole('button').element().textContent)
      .toContain(m.settings_clash_settings_managed_guard_field_unmanaged())
    await view.getByRole('button').click()
    await view
      .getByRole('menuitemcheckbox', {
        name: m.settings_clash_settings_managed_guard_field_enabled(),
        exact: true,
      })
      .click()
    const save =
      field === 'enable_tun_mode'
        ? backend.patchSettings
        : backend.patchOverrides
    const untouched =
      field === 'enable_tun_mode'
        ? backend.patchOverrides
        : backend.patchSettings
    await expect
      .poll(() => save.mock.lastCall)
      .toEqual([{ [field]: { kind: 'managed', value: true } }])
    expect(untouched).not.toHaveBeenCalled()
  },
)

test('the TUN selector leaves the separate stack unchanged when following the profile', async ({
  onTestFinished,
}) => {
  backend.value = { kind: 'managed', value: false }
  backend.patchSettings.mockResolvedValue(undefined)
  const view = await render(
    <ManagedGuardFieldSelector field="enable_tun_mode" />,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  await expect
    .poll(() => view.getByRole('button').element().textContent)
    .toContain(m.settings_clash_settings_managed_guard_field_disabled())
  await view.getByRole('button').click()
  await expect
    .element(
      view.getByRole('menuitemcheckbox', {
        name: m.settings_clash_settings_managed_guard_field_disabled(),
        exact: true,
      }),
    )
    .toBeChecked()
  await view
    .getByRole('menuitemcheckbox', {
      name: m.settings_clash_settings_managed_guard_field_unmanaged(),
      exact: true,
    })
    .click()
  await expect
    .poll(() => backend.patchSettings.mock.lastCall)
    .toEqual([{ enable_tun_mode: { kind: 'unmanaged' } }])
})
