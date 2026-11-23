import { createRoot } from 'react-dom/client'
import { expect, onTestFinished, test, vi } from 'vitest'
import '@/assets/styles/tailwind.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { WidgetId } from '@/components/widgets/consts'
import { WidgetOverlayHost, WidgetPreviewHost } from '@/components/widgets/core'
import '@/components/widgets/registry'
import { RpcProvider } from '@nyanpasu/query/provider'
import { createRpcClient } from '@nyanpasu/rpc'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { AllPermissions } from './all-permissions'

const types = Object.values(WidgetId)

test.each(['library', 'overlay'] as const)(
  'all new %s previews render without business queries or subscriptions',
  async (kind) => {
    const queried = vi.fn()
    const base = createRpcClient()
    const rpc = new Proxy(base, {
      get(target, key) {
        if (key === 'getStorageItem')
          return async () => ({
            status: 'ok',
            data: { value: null, revision: 0 },
          })
        if (key === 'listenResync') return () => () => {}
        if (key === 'events')
          return {
            ...base.events,
            storageValueChangedEvent: { listen: async () => () => {} },
            storageResyncEvent: { listen: async () => () => {} },
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
      <AllPermissions>
        <RpcProvider rpc={rpc}>
          <QueryClientProvider client={new QueryClient()}>
            <TooltipProvider>
              {types.map((type) => (
                <div
                  key={type}
                  data-preview={type}
                  style={{ width: 304, height: 304 }}
                >
                  {kind === 'library' ? (
                    <WidgetPreviewHost type={type} />
                  ) : (
                    <WidgetOverlayHost type={type} span={{ w: 4, h: 4 }} />
                  )}
                </div>
              ))}
            </TooltipProvider>
          </QueryClientProvider>
        </RpcProvider>
      </AllPermissions>,
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
