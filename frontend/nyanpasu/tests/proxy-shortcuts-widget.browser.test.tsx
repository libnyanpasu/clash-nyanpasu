import '@fontsource-variable/inter'
import '@/assets/styles/index.css'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { useState } from 'react'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import type { DashboardItem } from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import { WidgetId } from '@/components/widgets/widget-config'
import { ProxyShortcutsWidget } from '@/components/widgets/widget-shortcut'
import { DndContext } from '@dnd-kit/core'
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { TestQueryProvider } from './query-provider'

const items: DashboardItem[] = [
  { id: 'p', type: WidgetId.ProxyShortcuts, x: 0, y: 0, w: 3, h: 2 },
]
const ctl: { setW: (width: number) => void } = { setW: () => {} }

function Scene() {
  const [w, setW] = useState(255)
  ctl.setW = setW
  return (
    <BlockTaskProvider>
      <DashboardProvider>
        <DndContext>
          <DndGridProvider
            value={{
              displayItems: items,
              getItemRect: () => ({ left: 0, top: 0, width: w, height: 190 }),
              dropInfoMap: {},
              activeItemId: null,
              resizingItemId: null,
              disabled: true,
              sourceOnly: false,
              dragIdPrefix: '',
              isOverlay: false,
              constraintsMapRef: { current: {} },
              onResizeStart: () => {},
              onResizeMove: () => {},
              onResizeEnd: () => {},
            }}
          >
            <TooltipProvider>
              <div
                style={{ width: 600, height: 190, position: 'relative' }}
                id="host"
              >
                <ProxyShortcutsWidget id="p" />
              </div>
            </TooltipProvider>
          </DndGridProvider>
        </DndContext>
      </DashboardProvider>
    </BlockTaskProvider>
  )
}

test('the proxy status header settles instead of flickering in a narrow widget', async ({
  onTestFinished,
}) => {
  const invoke = async (method: string) => {
    switch (method) {
      case 'get_app_config':
        return { enable_system_proxy: false }
      case 'get_clash_config':
        return { enable_tun_mode: false }
      case 'get_sys_proxy':
        return { enable: false, server: ':0', bypass: '' }
      case 'clash_api_get_configs':
        return { 'mixed-port': 7890 }
      default:
        return null
    }
  }
  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenMutation: async () => () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }
  const rpc = createRpcClient({
    commands: { invoke: async (m: string) => invoke(m) as never },
    events,
  })
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
  })
  const rootRoute = createRootRoute({ component: Outlet })
  const route = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/dashboard',
    component: Scene,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ['/main/dashboard'] }),
  })
  const screen = await render(
    <TestQueryProvider client={queryClient} rpc={rpc}>
      <RouterProvider router={router} />
    </TestQueryProvider>,
  )
  onTestFinished(async () => {
    await screen.unmount()
    queryClient.clear()
  })
  await expect
    .poll(() => document.getElementById('host')?.textContent)
    .toContain('Disabled')
  await document.fonts.load('700 16px "Inter Variable"')
  const host = document.getElementById('host')!
  let mutations = 0
  const observer = new MutationObserver((records) => {
    mutations += records.filter((record) => {
      const slot = (record.target as HTMLElement).dataset?.slot
      return slot !== 'shape-toggle' && slot !== 'material-shape'
    }).length
  })
  observer.observe(host, {
    subtree: true,
    childList: true,
    attributes: true,
    characterData: true,
  })
  onTestFinished(() => observer.disconnect())

  // The header's two marquees compete for the width below 282 px, which once
  // switched them between scrolling and static on every frame.
  for (const width of [190, 220, 255]) {
    ctl.setW(width)
    await new Promise((resolve) => setTimeout(resolve, 100))
    mutations = 0
    await new Promise((resolve) => setTimeout(resolve, 200))

    expect(mutations, `${width}px`).toBe(0)
  }
})
