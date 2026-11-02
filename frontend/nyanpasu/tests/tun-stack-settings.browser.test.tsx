import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import type { ConfigFeatures } from '@nyanpasu/rpc/types'
import MipsCongestionControllerSelector from '../src/pages/(main)/main/settings/clash/network/_modules/mips-congestion-controller-selector'
import TunStackSelector from '../src/pages/(main)/main/settings/clash/network/_modules/tun-stack-selector'

const MIHOMO: ConfigFeatures = {
  tun: {
    stack: { kind: 'values', value: ['system', 'gvisor', 'mixed', 'mips'] },
    congestion_controller: {
      kind: 'values',
      value: ['cubic', 'reno', 'bbr', 'bbr3'],
    },
  },
}

const backend = vi.hoisted(() => ({
  features: undefined as ConfigFeatures | undefined,
  settings: {} as Record<string, unknown>,
  upsert: vi.fn(),
  refetch: vi.fn(),
}))
vi.mock('@nyanpasu/query', () => ({
  useConfigFeatures: () => ({ data: backend.features }),
  useClashSetting: (key: string) => ({
    value: backend.settings[key],
    upsert: (value: unknown) => backend.upsert(key, value),
  }),
  useRuntimeProfile: () => ({ refetch: backend.refetch }),
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

function setup(
  features: ConfigFeatures,
  settings: Record<string, unknown>,
  onTestFinished: (fn: () => Promise<void>) => void,
) {
  backend.features = features
  backend.settings = settings
  backend.upsert.mockResolvedValue(undefined)
  backend.refetch.mockResolvedValue(undefined)
  onTestFinished(async () => {
    vi.clearAllMocks()
  })
}

test('lists only the stacks the core reads and saves the selected one', async ({
  onTestFinished,
}) => {
  setup(
    {
      tun: {
        stack: { kind: 'values', value: ['system', 'gvisor'] },
        congestion_controller: { kind: 'ignored' },
      },
    },
    { tun_stack: 'gvisor', enable_tun_mode: false },
    onTestFinished,
  )
  const view = await render(<TunStackSelector />)
  onTestFinished(() => view.unmount())

  await view
    .getByRole('button', {
      name: new RegExp(m.settings_clash_settings_tun_stack_label()),
    })
    .click()
  await expect
    .element(view.getByRole('menuitemcheckbox', { name: 'gVisor' }))
    .toBeChecked()
  expect(view.getByRole('menuitemcheckbox').elements()).toHaveLength(2)

  await view.getByRole('menuitemcheckbox', { name: 'System' }).click()
  await expect
    .poll(() => backend.upsert.mock.lastCall)
    .toEqual(['tun_stack', 'system'])
})

test('hides the stack for a core that does not read it', async ({
  onTestFinished,
}) => {
  setup(
    {
      tun: {
        stack: { kind: 'ignored' },
        congestion_controller: { kind: 'ignored' },
      },
    },
    { tun_stack: 'mixed' },
    onTestFinished,
  )
  const view = await render(
    <>
      <TunStackSelector />
      <MipsCongestionControllerSelector />
    </>,
  )
  onTestFinished(() => view.unmount())

  expect(view.container.childElementCount).toBe(0)
})

test('offers the congestion controller only on the mips stack', async ({
  onTestFinished,
}) => {
  setup(MIHOMO, { tun_stack: 'gvisor', tun_mips: {} }, onTestFinished)
  const view = await render(<MipsCongestionControllerSelector />)
  onTestFinished(() => view.unmount())

  expect(view.container.childElementCount).toBe(0)
})

test('sets the congestion controller and hands it back to the core', async ({
  onTestFinished,
}) => {
  setup(
    MIHOMO,
    { tun_stack: 'mips', tun_mips: { congestion_controller: 'bbr' } },
    onTestFinished,
  )
  const view = await render(<MipsCongestionControllerSelector />)
  onTestFinished(() => view.unmount())

  await view
    .getByRole('button', {
      name: new RegExp(
        m.settings_clash_settings_tun_congestion_controller_label(),
      ),
    })
    .click()
  await expect
    .element(view.getByRole('menuitemcheckbox', { name: 'BBR', exact: true }))
    .toBeChecked()

  await view
    .getByRole('menuitemcheckbox', {
      name: m.settings_clash_settings_tun_congestion_controller_core_default(),
    })
    .click()
  await expect
    .poll(() => backend.upsert.mock.lastCall)
    .toEqual(['tun_mips', { congestion_controller: null }])
})
