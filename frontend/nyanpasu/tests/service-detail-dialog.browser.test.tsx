import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import SystemServiceCtrl from '../src/pages/(main)/main/settings/system/_modules/system-service-ctrl'

const service = vi.hoisted(() => ({
  name: 'nyanpasu-service',
  version: '2.0.3',
  status: 'running',
  phase: 'ready',
  compat: { kind: 'compatible', server_version: '2.0.3' },
  restart_attempts: 1,
  server: {
    version: '2.0.3',
    core_infos: {
      type: { clash: 'mihomo' },
      state: 'Running',
      state_changed_at: 0,
      config_path: '/var/lib/nyanpasu-service/clash-config.yaml',
      controller: { UnixSocket: '/var/run/nyanpasu-service/mihomo.sock' },
      health: {
        state: 'Healthy',
        changed_at: 0,
        consecutive_failures: 0,
        last_error: null,
        last_success_at: 0,
      },
    },
    runtime_infos: {
      service_data_dir: '/var/lib/nyanpasu-service',
      service_config_dir: '/etc/nyanpasu-service',
      nyanpasu_config_dir: '/home/user/.config/clash-nyanpasu',
      nyanpasu_data_dir: '/home/user/.local/share/clash-nyanpasu',
    },
    // Real payloads are long; the raw view must scroll inside the dialog.
    logs: {
      files: Array.from({ length: 200 }, (_, i) => `/var/log/core-${i}.log`),
    },
  },
}))

vi.mock('@nyanpasu/query', () => ({
  useSystemService: () => ({
    query: { data: service },
    upsert: { mutateAsync: vi.fn(), isPending: false },
  }),
  useServicePrompt: () => ({ data: undefined }),
  useCoreDir: () => ({ data: '/opt/clash-nyanpasu' }),
}))
vi.mock('@/services/rpc', () => ({ rpc: {}, commands: {} }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

test('service detail shows the key fields and fits the window', async ({
  onTestFinished,
}) => {
  await page.viewport(1280, 720)
  const view = await render(<SystemServiceCtrl />)
  onTestFinished(async () => {
    await view.unmount()
    await page.viewport(414, 896)
  })

  await page
    .elementLocator(
      view.container.querySelector(
        '[data-slot="system-service-detail-button"]',
      )!,
    )
    .click()

  const dialog = () =>
    document.querySelector<HTMLElement>('[data-slot="service-detail"]')
  const rawJson = () =>
    document.querySelector('[data-slot="service-detail-raw-json"]')

  await expect.poll(dialog).toBeTruthy()
  const text = dialog()!.textContent
  expect(text).toContain('Running')
  expect(text).toContain('Ready')
  expect(text).toContain('mihomo')
  expect(text).toContain('Healthy')
  expect(text).toContain('/var/run/nyanpasu-service/mihomo.sock')
  expect(text).toContain('/etc/nyanpasu-service')
  expect(rawJson()).toBeNull()

  // Wider than the old fixed w-96, still inside the window.
  const { width } = dialog()!.getBoundingClientRect()
  expect(width).toBeGreaterThan(384)
  expect(width).toBeLessThanOrEqual(1280 - 32)

  await page.getByRole('button', { name: 'Show Raw JSON' }).click()

  await expect.poll(() => rawJson()?.querySelector('.shiki')).toBeTruthy()
  expect(rawJson()!.textContent).toContain('/var/log/core-199.log')
  expect(dialog()!.getBoundingClientRect().height).toBeLessThanOrEqual(
    720 * 0.9,
  )
  const viewport = dialog()!.querySelector(
    '[data-slot="scroll-area-viewport"]',
  )!
  expect(viewport.scrollHeight).toBeGreaterThan(viewport.clientHeight)

  await page.getByRole('button', { name: 'Hide Raw JSON' }).click()
  await expect.poll(rawJson).toBeNull()

  await page.viewport(414, 896)
  await expect
    .poll(() => dialog()!.getBoundingClientRect().width)
    .toBeLessThanOrEqual(414 - 32)
})
