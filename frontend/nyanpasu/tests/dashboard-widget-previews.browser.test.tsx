import { createRoot } from 'react-dom/client'
import { expect, onTestFinished, test, vi } from 'vitest'
import '@/assets/styles/tailwind.css'
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

const types = Object.values(WidgetId)

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
      x: (index % 4) * 4,
      y: Math.floor(index / 4) * 4,
      w: 4,
      h: 4,
    }))
    const container = document.createElement('div')
    container.style.width = '1216px'
    container.style.height = '1280px'
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
                  getItemRect: (item) => ({
                    left: item.x * 76,
                    top: item.y * 80,
                    width: item.w * 76,
                    height: item.h * 80 - 16,
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
                    <div
                      key={item.id}
                      data-preview={item.type}
                      style={
                        kind === 'overlay'
                          ? { width: 304, height: 304 }
                          : undefined
                      }
                    >
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
      .toBe(types.length)
    expect(container.querySelectorAll('.animate-pulse')).toHaveLength(0)
    for (const selector of [
      '[data-slot="subscription-quota-wave"]',
      '[data-slot="subscription-quota-percent"]',
      '[data-slot="subscription-schedule-preview"]',
      '[data-slot="widget-traffic-report-total"]',
      '[data-slot="widget-traffic-report-featured"]',
      '[data-slot="widget-active-connections-row"]',
      '[data-slot="widget-provider-updates-preview-row"]',
    ]) {
      expect(container.querySelector(selector), selector).not.toBeNull()
    }
    expect(
      container.querySelectorAll('[data-slot="widget-active-connections-row"]'),
    ).toHaveLength(2)
    expect(
      container.querySelectorAll(
        '[data-slot="widget-provider-updates-preview-row"]',
      ),
    ).toHaveLength(2)
    for (const [type, sampleName] of [
      [WidgetId.OriginTraffic, 'Safari'],
      [WidgetId.ExitTraffic, 'Japan Premium'],
      [WidgetId.TargetTraffic, 'video.example.com'],
      [WidgetId.RuleTraffic, 'DOMAIN-SUFFIX,example.com'],
    ] as const) {
      const preview = container.querySelector(`[data-preview="${type}"]`)
      const featuredSlot =
        type === WidgetId.ExitTraffic
          ? 'widget-exit-traffic-featured'
          : 'widget-traffic-report-featured'
      const rowSlot =
        type === WidgetId.ExitTraffic
          ? 'widget-exit-traffic-row'
          : 'widget-traffic-report-row'
      expect(
        preview?.querySelector(`[data-slot="${featuredSlot}"]`),
      ).not.toBeNull()
      await expect
        .poll(() => preview?.querySelector(`[data-slot="${rowSlot}"]`))
        .not.toBeNull()
      expect(preview?.querySelector('[role="progressbar"]')).not.toBeNull()
      expect(preview?.textContent).toContain(sampleName)
    }
    expect(
      container.querySelector('[data-preview="active-connections"]')
        ?.textContent,
    ).toContain('video.example.com')
    expect(
      container.querySelector('[data-preview="provider-updates"]')?.textContent,
    ).toContain('Japan Premium')
    expect(
      container.querySelector(
        '[data-preview="proxy-mode"] [data-slot="expressive-choice-options"]',
      ),
    ).not.toBeNull()
    expect(
      container.querySelector(
        '[data-preview="proxy-shortcuts"] [data-slot="proxy-shortcut-buttons"]',
      ),
    ).not.toBeNull()
    expect(
      container.querySelector(
        '[data-preview="core-shortcuts"] [data-slot="current-core-card"]',
      ),
    ).not.toBeNull()
    for (const type of [
      WidgetId.TrafficDown,
      WidgetId.TrafficUp,
      WidgetId.Connections,
      WidgetId.Memory,
    ]) {
      const preview = container.querySelector(`[data-preview="${type}"]`)
      expect(
        preview?.querySelector('[data-slot="widget-sparkline-card"]'),
      ).not.toBeNull()
      await expect
        .poll(() => preview?.querySelector('svg path[d]:not([d=""])'))
        .not.toBeNull()
    }
    expect(queried).not.toHaveBeenCalled()
    expect(
      container.querySelector('[data-slot="widget-config-trigger"]'),
    ).toBeNull()
  },
)
