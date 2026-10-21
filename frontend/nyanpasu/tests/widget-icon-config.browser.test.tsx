import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import NetworkStatisticWidgetIconConfig from '../src/pages/(main)/main/settings/nyanpasu/widget/_modules/network-statistic-widget-icon-config'

const state = vi.hoisted(() => ({
  upload: vi.fn(),
  mutations: {
    setTrayIcon: { name: 'setTrayIcon' },
    setTrayIconFromBytes: { name: 'setTrayIconFromBytes' },
    setWidgetIcon: { name: 'setWidgetIcon' },
    setWidgetIconFromBytes: { name: 'setWidgetIconFromBytes' },
  },
}))
// The tray offers no presets here; the widget's own set still does.
vi.mock('@nyanpasu/platform', () => ({
  isWindows: false,
}))
vi.mock('@/components/ui/image', () => ({
  TrayImage: () => <span />,
  WidgetImage: () => <span />,
}))
vi.mock('@/utils/file-picker', () => ({ pickFile: vi.fn() }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@/services/rpc', () => ({
  queries: {
    isWidgetIconSet: (mode: string) => ({
      queryKey: ['isWidgetIconSet', mode],
      queryFn: async () => ({ status: 'ok', data: false }),
    }),
  },
  mutations: state.mutations,
}))
vi.mock('@nyanpasu/query', async (original) => ({
  ...(await original<typeof import('@nyanpasu/query')>()),
  useWidgetIcon: () => ({ data: undefined }),
  invokeMutation: (mutation: unknown, args: unknown[]) =>
    state.upload(mutation, ...args),
}))

test('the widget presets write the widget icons, not the tray icons', async ({
  onTestFinished,
}) => {
  state.upload.mockReset().mockResolvedValue(undefined)
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const invalidate = vi.spyOn(client, 'invalidateQueries')
  const view = await render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <NetworkStatisticWidgetIconConfig />
      </TooltipProvider>
    </QueryClientProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
  })

  await expect
    .element(
      view.getByRole('radiogroup', {
        name: m.settings_nyanpasu_network_statistic_widget_logo_label(),
      }),
    )
    .toBeVisible()
  await view
    .getByRole('radio', {
      name: new RegExp(m.settings_nyanpasu_tray_icon_preset_mascot()),
    })
    .click()

  await expect.poll(() => state.upload.mock.calls.length).toBe(3)
  expect(
    state.upload.mock.calls.map(([mutation, mode]) => [mutation, mode]),
  ).toEqual([
    [state.mutations.setWidgetIconFromBytes, 'normal'],
    [state.mutations.setWidgetIconFromBytes, 'tun'],
    [state.mutations.setWidgetIconFromBytes, 'system_proxy'],
  ])
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ['getWidgetIcon', 'tun'],
  })
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ['isWidgetIconSet', 'tun'],
  })
})
