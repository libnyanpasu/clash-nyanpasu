import { createRoot } from 'react-dom/client'
import { expect, onTestFinished, test, vi } from 'vitest'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import {
  RENDER_MAP,
  WidgetId,
  type DashboardItem,
} from '@/components/widgets/consts'
import { DashboardProvider } from '@/components/widgets/provider'
import { DndContext } from '@dnd-kit/core'
import { RpcProvider } from '@nyanpasu/query/provider'
import { createRpcClient } from '@nyanpasu/rpc'

const types = [
  WidgetId.SubscriptionQuota,
  WidgetId.SubscriptionSchedule,
  WidgetId.ProfileShortcuts,
  WidgetId.ProxyMode,
  WidgetId.RecentTraffic,
  WidgetId.OriginTraffic,
  WidgetId.ExitTraffic,
  WidgetId.TargetTraffic,
  WidgetId.RuleTraffic,
  WidgetId.ActiveConnections,
  WidgetId.ConfigurationHealth,
  WidgetId.ProviderUpdates,
]

test.each(['library', 'overlay'] as const)(
  'all new %s previews render without business queries or subscriptions',
  async (kind) => {
    const queried = vi.fn()
    const base = createRpcClient()
    const rpc = new Proxy(base, {
      get(target, key) {
        if (key === 'getStorageItem')
          return async () => ({ status: 'ok', data: null })
        if (key === 'listenResync') return () => () => {}
        if (key === 'events')
          return {
            ...base.events,
            storageValueChangedEvent: { listen: async () => () => {} },
          }
        const value = Reflect.get(target, key)
        return typeof value === 'function'
          ? (...args: unknown[]) => {
              queried(key, ...args)
              throw new Error(`Unexpected preview RPC: ${String(key)}`)
            }
          : value
      },
    })
    const items: DashboardItem[] = types.map((type, index) => ({
      id: type,
      type,
      x: 0,
      y: index * 3,
      w: 4,
      h: 3,
    }))
    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)
    onTestFinished(() => {
      root.unmount()
      container.remove()
    })
    root.render(
      <RpcProvider rpc={rpc}>
        <TooltipProvider>
          <DashboardProvider>
            <DndContext>
              <DndGridProvider
                value={{
                  displayItems: items,
                  getItemRect: () => ({
                    left: 0,
                    top: 0,
                    width: 304,
                    height: 224,
                  }),
                  dropInfoMap: {},
                  activeItemId: null,
                  resizingItemId: null,
                  disabled: false,
                  sourceOnly: kind === 'library',
                  isOverlay: kind === 'overlay',
                  dragIdPrefix: kind === 'library' ? 'sheet:' : '',
                  constraintsMapRef: { current: {} },
                  onResizeStart: () => {},
                  onResizeMove: () => {},
                  onResizeEnd: () => {},
                }}
              >
                {items.map((item) => {
                  const Widget = RENDER_MAP[item.type]
                  return (
                    <div key={item.id} data-preview={item.type}>
                      <Widget id={item.id} />
                    </div>
                  )
                })}
              </DndGridProvider>
            </DndContext>
          </DashboardProvider>
        </TooltipProvider>
      </RpcProvider>,
    )
    await expect
      .poll(
        () =>
          [...container.querySelectorAll('[data-preview]')].filter((node) =>
            node.textContent?.trim(),
          ).length,
      )
      .toBe(12)
    expect(queried).not.toHaveBeenCalled()
    expect(
      container.querySelector('[data-slot="widget-config-trigger"]'),
    ).toBeNull()
  },
)
