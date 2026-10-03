import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import UpdatePreferencesCard from '../src/pages/(main)/main/settings/about/_modules/update-preferences-card'

const state = vi.hoisted(() => ({
  autoCheck: true,
  autoDownload: false,
  loaded: true,
  channel: 'stable',
  installedChannel: 'stable',
  upsert: vi.fn(),
  changeChannel: vi.fn(),
}))

vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => ({
    value: !state.loaded
      ? undefined
      : key === 'enable_auto_check_update'
        ? state.autoCheck
        : state.autoDownload,
    isPending: false,
    upsert: state.upsert,
  }),
  useReleaseChannel: () => ({
    query: {
      data: { current: state.channel, installed: state.installedChannel },
    },
    mutation: { mutateAsync: state.changeChannel, isPending: false },
  }),
}))
vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({ isBusy: false }),
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => true,
}))

test('automatic downloads default off and require automatic checks', async ({
  onTestFinished,
}) => {
  state.autoCheck = true
  state.autoDownload = false
  state.upsert.mockReset()
  const view = await render(<UpdatePreferencesCard />)
  onTestFinished(async () => view.unmount())

  const autoCheck = view.getByRole('switch', {
    name: m.settings_label_about_auto_check_updates(),
  })
  const autoDownload = view.getByRole('switch', {
    name: m.settings_about_auto_download_label(),
  })
  await expect.element(autoCheck).toBeChecked()
  await expect.element(autoDownload).not.toBeChecked()
  await expect.element(autoDownload).toBeEnabled()

  state.autoCheck = false
  await view.rerender(<UpdatePreferencesCard />)
  await expect.element(autoDownload).toBeDisabled()
  expect(state.upsert).not.toHaveBeenCalled()
})

test('release channel is part of update preferences and can be changed', async ({
  onTestFinished,
}) => {
  state.channel = 'stable'
  state.installedChannel = 'stable'
  state.changeChannel.mockReset()
  const view = await render(<UpdatePreferencesCard />)
  onTestFinished(async () => view.unmount())

  const channel = view.getByRole('button', {
    name: m.release_channel_label(),
  })
  await expect
    .element(channel.getByText(m.release_channel_stable(), { exact: true }))
    .toBeVisible()
  await channel.click()
  await view
    .getByRole('menuitemcheckbox', { name: m.release_channel_beta() })
    .click()

  expect(state.changeChannel).toHaveBeenCalledWith('beta')
})

test('preferences remain disabled until configuration is available', async ({
  onTestFinished,
}) => {
  state.loaded = false
  state.upsert.mockReset()
  const view = await render(<UpdatePreferencesCard />)
  onTestFinished(async () => {
    await view.unmount()
    state.loaded = true
  })
  await expect
    .element(
      view.getByRole('switch', {
        name: m.settings_label_about_auto_check_updates(),
      }),
    )
    .toBeDisabled()
  await expect
    .element(
      view.getByRole('switch', {
        name: m.settings_about_auto_download_label(),
      }),
    )
    .toBeDisabled()
  expect(state.upsert).not.toHaveBeenCalled()
})
