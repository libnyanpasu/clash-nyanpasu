import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import catNormal from '@root/backend/tauri/icons/tray/cat/normal.png'
import catSystemProxy from '@root/backend/tauri/icons/tray/cat/system-proxy.png'
import catTun from '@root/backend/tauri/icons/tray/cat/tun.png'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import TrayIconConfig from '../src/pages/(main)/main/settings/nyanpasu/_modules/tray-icon-config'

const state = vi.hoisted(() => ({
  upload: vi.fn(),
  icons: undefined as Record<string, string> | undefined,
}))
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
  useTrayIcon: (mode: string) => ({
    data: state.icons ? { data_url: state.icons[mode] } : undefined,
  }),
  invokeMutation: (_mutation: unknown, args: unknown[]) =>
    state.upload(...args),
}))

const PNG_SIGNATURE = '\x89PNG\r\n\x1a\n'

async function setup(
  onTestFinished: (callback: () => Promise<void>) => void,
  icons?: Record<string, string>,
) {
  state.icons = icons
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

type View = Awaited<ReturnType<typeof setup>>['view']

const presetRadio = (view: View, label: string) =>
  view.getByRole('radio', { name: new RegExp(label) })

const customRadio = (view: View) =>
  presetRadio(view, m.settings_nyanpasu_tray_icon_custom())

const libraryIcon = (id: string) =>
  document.querySelector<HTMLElement>(
    `[data-slot="tray-icon-library-item"][data-icon="${id}"]`,
  )

const slot = (mode: string) =>
  document.querySelector<HTMLElement>(
    `[data-slot="tray-icon-slot"][data-mode="${mode}"]`,
  )!

const centerOf = (element: Element) => {
  const rect = element.getBoundingClientRect()

  return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

// Presses, holds past the pointer activation delay, moves onto the target (or
// far from every slot) and releases. `cancel` presses Escape before releasing.
async function dragIcon(
  source: Element,
  target: Element | null,
  cancel = false,
) {
  const start = centerOf(source)
  const end = target ? centerOf(target) : { x: start.x, y: start.y - 800 }
  const init = (point: { x: number; y: number }) => ({
    bubbles: true,
    cancelable: true,
    pointerId: 1,
    pointerType: 'mouse',
    isPrimary: true,
    button: 0,
    buttons: 1,
    clientX: point.x,
    clientY: point.y,
  })

  source.dispatchEvent(new PointerEvent('pointerdown', init(start)))
  await sleep(300)

  for (const t of [0.25, 0.5, 0.75, 1]) {
    document.dispatchEvent(
      new PointerEvent(
        'pointermove',
        init({
          x: start.x + (end.x - start.x) * t,
          y: start.y + (end.y - start.y) * t,
        }),
      ),
    )
    await sleep(30)
  }

  if (cancel) {
    await userEvent.keyboard('{Escape}')
  }
  document.dispatchEvent(new PointerEvent('pointerup', init(end)))
}

test('a Windows preset uploads three real PNGs and locks controls until completion', async ({
  onTestFinished,
}) => {
  const { view, invalidate } = await setup(onTestFinished)
  await page.viewport(320, 640)
  const config = document.querySelector('[data-slot="tray-icon-config"]')!
  expect(config.scrollWidth).toBeLessThanOrEqual(config.clientWidth)
  let finishFirst!: () => void
  state.upload.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finishFirst = resolve
      }),
  )
  const mascot = presetRadio(
    view,
    m.settings_nyanpasu_tray_icon_preset_mascot(),
  )
  const cat = presetRadio(view, m.settings_nyanpasu_tray_icon_preset_cat())
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
    expect(atob(base64).slice(0, 8)).toBe(PNG_SIGNATURE)
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
  const cat = presetRadio(view, m.settings_nyanpasu_tray_icon_preset_cat())
  await cat.click()
  await expect.poll(() => state.upload.mock.calls.length).toBe(2)
  await expect.element(cat).toBeEnabled()
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ['isTrayIconSet', 'normal'],
  })
})

test('the preset matching the current icons is shown as selected', async ({
  onTestFinished,
}) => {
  const { view } = await setup(onTestFinished, {
    normal: catNormal,
    tun: catTun,
    system_proxy: catSystemProxy,
  })
  await expect
    .element(presetRadio(view, m.settings_nyanpasu_tray_icon_preset_cat()))
    .toBeChecked()
  await expect
    .element(presetRadio(view, m.settings_nyanpasu_tray_icon_preset_mascot()))
    .not.toBeChecked()
})

test('only the custom mode exposes the icon library and upload actions', async ({
  onTestFinished,
}) => {
  const { view } = await setup(onTestFinished, {
    normal: catNormal,
    tun: catTun,
    system_proxy: catSystemProxy,
  })

  await expect.poll(() => view.getByRole('radio').elements().length).toBe(3)
  await expect
    .element(presetRadio(view, m.settings_nyanpasu_tray_icon_preset_cat()))
    .toBeChecked()
  expect(document.querySelector('[data-slot="tray-icon-library"]')).toBeNull()
  expect(
    view.getByText(m.settings_nyanpasu_tray_icon_upload()).elements(),
  ).toHaveLength(0)

  // Entering custom writes nothing, and a mix that still equals the preset
  // stays editable instead of snapping back to the preset.
  await customRadio(view).click()
  await expect.element(customRadio(view)).toBeChecked()
  await expect
    .poll(
      () =>
        document.querySelectorAll('[data-slot="tray-icon-library-item"]')
          .length,
    )
    .toBe(6)
  expect(
    view.getByText(m.settings_nyanpasu_tray_icon_upload()).elements(),
  ).toHaveLength(3)
  expect(state.upload).not.toHaveBeenCalled()
})

test('clicking a library icon applies it to the selected state only', async ({
  onTestFinished,
}) => {
  const { view, invalidate } = await setup(onTestFinished)
  await customRadio(view).click()
  await expect.poll(() => libraryIcon('mascot-system_proxy')).not.toBeNull()

  slot('tun').querySelector('button')!.click()
  await expect
    .poll(() => slot('tun').querySelector('button')!.ariaPressed)
    .toBe('true')
  libraryIcon('mascot-system_proxy')!.click()

  await expect.poll(() => state.upload.mock.calls.length).toBe(1)
  expect(state.upload.mock.calls[0][0]).toBe('tun')
  expect(atob(state.upload.mock.calls[0][1]).slice(0, 8)).toBe(PNG_SIGNATURE)
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['getTrayIcon', 'tun'] })
  expect(invalidate).not.toHaveBeenCalledWith({
    queryKey: ['getTrayIcon', 'normal'],
  })
})

test('dropping a library icon on a state writes once; cancelling or missing writes nothing', async ({
  onTestFinished,
}) => {
  const { view } = await setup(onTestFinished)
  await customRadio(view).click()
  await expect.poll(() => libraryIcon('cat-tun')).not.toBeNull()

  await dragIcon(libraryIcon('cat-tun')!, slot('normal'), true)
  await sleep(400)
  expect(state.upload, 'after cancel').not.toHaveBeenCalled()
  await dragIcon(libraryIcon('cat-tun')!, null)
  await sleep(400)
  expect(state.upload).not.toHaveBeenCalled()

  await dragIcon(libraryIcon('cat-tun')!, slot('system_proxy'))
  await expect.poll(() => state.upload.mock.calls.length).toBe(1)
  expect(state.upload.mock.calls[0][0]).toBe('system_proxy')
})

test('Space picks an icon up for keyboard dragging, Escape cancels, and Enter applies', async ({
  onTestFinished,
}) => {
  const { view } = await setup(onTestFinished)
  await customRadio(view).click()
  await expect.poll(() => libraryIcon('cat-normal')).not.toBeNull()

  libraryIcon('cat-normal')!.focus()
  await userEvent.keyboard(' ')
  await userEvent.keyboard('{Escape}')
  await sleep(300)
  expect(state.upload).not.toHaveBeenCalled()

  // Enter is the click path: it applies the icon to the selected state.
  await userEvent.keyboard('{Enter}')
  await expect.poll(() => state.upload.mock.calls.length).toBe(1)
  expect(state.upload.mock.calls[0][0]).toBe('normal')
})
