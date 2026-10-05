import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import CoreManagerCard from '../src/pages/(main)/main/settings/clash/_modules/core-manager-card'

const state = vi.hoisted(() => ({
  currentCore: 'mihomo',
  cores: {
    mihomo: {
      name: 'Mihomo',
      currentVersion: 'N/A',
      latestVersion: 'v1.19.32',
      versionReadError: true,
    },
    'meow-alpha': {
      name: 'Meow Alpha',
      currentVersion: 'N/A',
      latestVersion: 'alpha-3c27aca',
      versionReadError: true,
    },
  },
  updateCore: vi.fn(async () => 1),
  inspectUpdater: vi.fn(async () => ({
    id: 1,
    state: 'done',
    downloader: { state: 'idle', downloaded: 0, total: 0, speed: 0 },
  })),
  refetchVersions: vi.fn(async () => state.cores),
  message: vi.fn(),
}))

vi.mock('@nyanpasu/query', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/query')>()),
  useClashCores: () => ({
    query: { data: state.cores, isPending: false },
    updateCore: { mutateAsync: state.updateCore, isPending: false },
    inspectUpdater: state.inspectUpdater,
    refetchVersions: state.refetchVersions,
    upsert: { mutateAsync: vi.fn(), isPending: false },
    restartSidecar: vi.fn(),
    fetchRemote: { mutateAsync: vi.fn(), isPending: false },
  }),
  useDeleteClashConnections: () => ({ mutateAsync: vi.fn() }),
  useSetting: () => ({ value: state.currentCore, upsert: vi.fn() }),
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: state.message }))
vi.mock('@/paraglide/messages', () => ({
  m: new Proxy({}, { get: (_, key: string) => () => key }),
}))

test('both core card entries expose a reinstall action after a failed version read', async ({
  onTestFinished,
}) => {
  state.updateCore.mockClear()
  state.inspectUpdater.mockClear()
  state.message.mockClear()

  const view = await render(
    <TooltipProvider>
      <BlockTaskProvider>
        <CoreManagerCard />
      </BlockTaskProvider>
    </TooltipProvider>,
  )
  onTestFinished(async () => view.unmount())

  await expect
    .poll(
      () =>
        view.container.querySelectorAll(
          `[aria-label="${m.settings_clash_core_manager_card_reinstall_core()}"]`,
        ).length,
    )
    .toBe(2)
  await expect
    .poll(() => view.container.querySelectorAll(`span.text-error`).length)
    .toBe(2)

  await view
    .getByLabelText(m.settings_clash_core_manager_card_reinstall_core())
    .nth(0)
    .click()
  await expect.poll(() => state.updateCore.mock.calls.length).toBe(1)
  expect(state.updateCore).toHaveBeenCalledWith('mihomo')
})

test('equal versions do not add update actions to either core card entry', async ({
  onTestFinished,
}) => {
  state.currentCore = 'mihomo'
  state.cores = {
    mihomo: {
      name: 'Mihomo',
      currentVersion: 'v0.22.0',
      latestVersion: '0.22.0',
      versionReadError: false,
    },
    'meow-alpha': {
      name: 'Meow Alpha',
      currentVersion: '0.22.0-alpha+3c27aca',
      latestVersion: 'alpha-3c27aca',
      versionReadError: false,
    },
  }
  const view = await render(
    <TooltipProvider>
      <BlockTaskProvider>
        <CoreManagerCard />
      </BlockTaskProvider>
    </TooltipProvider>,
  )
  onTestFinished(async () => view.unmount())

  expect(
    view.container.querySelectorAll(
      `[aria-label="${m.settings_clash_core_manager_card_click_to_update()}"]`,
    ),
  ).toHaveLength(0)
})
