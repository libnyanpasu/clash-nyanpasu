import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import TrayIconConfig from '../src/pages/(main)/main/settings/nyanpasu/_modules/tray-icon-config'

const state = vi.hoisted(() => ({ upload: vi.fn() }))
vi.mock('@nyanpasu/platform', () => ({
  isWindows: true,
}))
vi.mock('@/components/ui/image', () => ({ TrayImage: () => <span /> }))
vi.mock('@/utils/file-picker', () => ({ pickFile: vi.fn() }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@/services/rpc', () => ({
  queries: {
    isTrayIconSet: (mode: string) => ({
      queryKey: ['isTrayIconSet', mode],
      queryFn: async () => ({ status: 'ok', data: false }),
    }),
  },
  mutations: { setTrayIconFromBytes: {}, setTrayIcon: {} },
}))
vi.mock('@nyanpasu/query', async (original) => ({
  ...(await original<typeof import('@nyanpasu/query')>()),
  invokeMutation: (_mutation: unknown, args: unknown[]) =>
    state.upload(...args),
}))

async function setup(onTestFinished: (callback: () => Promise<void>) => void) {
  state.upload.mockReset().mockResolvedValue(undefined)
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const invalidate = vi.spyOn(client, 'invalidateQueries')
  const view = await render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <TrayIconConfig />
      </TooltipProvider>
    </QueryClientProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
  })

  return { view, invalidate }
}

test('a Windows preset uploads three real PNGs and locks controls until completion', async ({
  onTestFinished,
}) => {
  const { view, invalidate } = await setup(onTestFinished)
  await page.viewport(320, 640)
  const presets = document.querySelector('[data-slot="tray-icon-presets"]')!
  expect(presets.scrollWidth).toBeLessThanOrEqual(presets.clientWidth)
  let finishFirst!: () => void
  state.upload.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finishFirst = resolve
      }),
  )
  const mascot = view.getByRole('button', {
    name: new RegExp(m.settings_nyanpasu_tray_icon_preset_mascot()),
  })
  const cat = view.getByRole('button', {
    name: new RegExp(m.settings_nyanpasu_tray_icon_preset_cat()),
  })
  await mascot.click()
  await expect.poll(() => state.upload.mock.calls.length).toBe(1)
  await expect.element(cat).toBeDisabled()
  finishFirst()
  await expect.poll(() => state.upload.mock.calls.length).toBe(3)
  await expect.element(cat).toBeEnabled()
  expect(state.upload.mock.calls.map(([mode]) => mode)).toEqual([
    'normal',
    'tun',
    'system_proxy',
  ])
  for (const [, base64] of state.upload.mock.calls) {
    expect(atob(base64).slice(0, 8)).toBe('\x89PNG\r\n\x1a\n')
    expect(atob(base64).length).toBeLessThan(1024 * 1024)
  }
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['getTrayIcon', 'tun'] })
})

test('a failed write refreshes partial results and releases the controls', async ({
  onTestFinished,
}) => {
  const { view, invalidate } = await setup(onTestFinished)
  state.upload
    .mockResolvedValueOnce(undefined)
    .mockRejectedValueOnce(new Error('disk full'))
  const cat = view.getByRole('button', {
    name: new RegExp(m.settings_nyanpasu_tray_icon_preset_cat()),
  })
  await cat.click()
  await expect.poll(() => state.upload.mock.calls.length).toBe(2)
  await expect.element(cat).toBeEnabled()
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ['isTrayIconSet', 'normal'],
  })
})
