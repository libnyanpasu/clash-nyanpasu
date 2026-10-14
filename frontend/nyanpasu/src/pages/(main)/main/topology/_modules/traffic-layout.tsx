import {
  useMemo,
  useState,
  type PropsWithChildren,
  type ReactNode,
} from 'react'
import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@nyanpasu/ui/navigation-sidebar'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { SidebarProvider } from '@nyanpasu/ui/slider-sidebar'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { keepReturn } from '@/components/router/cross-navigation'
import { useMockTrafficNow } from '@/hooks/use-mock-traffic'
import { m } from '@/paraglide/messages'
import { usageAmount } from '@/utils/traffic-usage'
import { useLocalIpProbe, useSetting, useTrafficReport } from '@nyanpasu/query'
import { type ReportRequest } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import {
  DataPage,
  dataPageLayoutVariants,
  DataPageSurface,
  useDataPageBottomInset,
} from '../../_modules/data-page'
import { useUsageLabelOf } from '../../_modules/use-usage-label-of'
import { mockTrafficReport } from './mock-traffic'
import Notice from './notice'
import {
  pollInterval,
  toQuery,
  toTopologyRequest,
  type TrafficSearch,
} from './search'
import {
  RANKINGS,
  TRAFFIC_PAGES,
  TrafficPageContext,
  withoutTrafficView,
  type TrafficView,
} from './traffic-context'
import TrafficToolbar from './traffic-toolbar'

const RANKING_LIMIT = 5

/**
 * Fills the scroll viewport for short animated pages and grows for long ones.
 * The content must not flex-shrink, or its bottom dock padding is lost to overflow.
 */
function TrafficSurface({ children }: PropsWithChildren) {
  const bottomInset = useDataPageBottomInset()

  return (
    <DataPageSurface>
      {/* Radix inserts a content wrapper: carry the viewport height through it
          so short routes fill the surface throughout the outlet transition. */}
      <ScrollArea className="min-h-0 flex-1 overflow-clip [&>[data-slot=scroll-area-viewport]>div]:flex! [&>[data-slot=scroll-area-viewport]>div]:min-h-full! [&>[data-slot=scroll-area-viewport]>div]:flex-col">
        <div
          className="@container relative mx-auto flex min-h-full w-full max-w-7xl flex-none flex-col gap-3 p-3 md:p-4"
          data-slot="traffic-content"
          style={{ paddingBottom: bottomInset }}
        >
          {children}
        </div>
      </ScrollArea>
    </DataPageSurface>
  )
}

export default function TrafficLayout({
  view,
  search,
  onSearchChange,
  onViewConnections,
  toolbarStart,
}: {
  view: TrafficView
  search: TrafficSearch
  onSearchChange: (update: Partial<TrafficSearch>) => void
  onViewConnections?: () => void
  toolbarStart?: ReactNode
}) {
  // The parent layout stays mounted across child routes, preserving pause state.
  const [paused, setPaused] = useState(false)
  const { value: retention } = useSetting('traffic_retention')
  const labelOf = useUsageLabelOf()
  const { range, scope, filters, layers, metric, limit } = search

  const query = useMemo(
    () => toQuery({ range, scope, filters }),
    [range, scope, filters],
  )
  const request = useMemo<ReportRequest>(
    () => ({
      query,
      metric,
      rankings: RANKINGS.map(({ dimension }) => dimension),
      ranking_limit: RANKING_LIMIT,
      topology: toTopologyRequest({ view, layers, limit }),
    }),
    [query, metric, view, layers, limit],
  )

  const mockNow = useMockTrafficNow(paused)
  useLocalIpProbe(mockNow === null)

  const { data: recorded, isError: failed } = useTrafficReport(request, {
    refetchInterval: paused ? false : pollInterval(range),
    enabled: mockNow === null,
  })
  const mocked = useMemo(
    () => (mockNow === null ? undefined : mockTrafficReport(request, mockNow)),
    [request, mockNow],
  )
  const report = mocked ?? recorded
  const isError = !mocked && failed
  const empty =
    !!report &&
    report.total.connections === 0 &&
    report.total.bytes.upload + report.total.bytes.download === 0
  const title = TRAFFIC_PAGES.find((page) => page.view === view)!.title()

  return (
    <TrafficPageContext
      value={{
        search,
        report,
        query,
        labelOf,
        onSearchChange,
        empty,
        retention,
        paused,
        onPausedChange: setPaused,
        onViewConnections,
      }}
    >
      <SidebarProvider defaultOpen={false}>
        <div className={dataPageLayoutVariants()} data-slot="traffic-page">
          <NavigationSidebar aria-label={m.traffic_sidebar()}>
            <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
              <NavigationSidebarContent className="pb-16">
                {TRAFFIC_PAGES.map(({ view: item, to, icon: Icon, title }) => (
                  <NavigationSidebarItem
                    key={item}
                    active={view === item}
                    icon={<Icon />}
                    renderLink={(content) => (
                      <Link
                        from="/main/topology"
                        to={to}
                        search={withoutTrafficView}
                        state={keepReturn}
                      >
                        {content}
                      </Link>
                    )}
                  >
                    {title()}
                  </NavigationSidebarItem>
                ))}
              </NavigationSidebarContent>
            </ScrollArea>
            <NavigationSidebarToggle aria-label={m.traffic_sidebar()}>
              {m.navigation_sidebar_collapse()}
            </NavigationSidebarToggle>
          </NavigationSidebar>
          <DataPage
            title={m.topology_title()}
            toolbar={<TrafficToolbar>{toolbarStart}</TrafficToolbar>}
          >
            <TrafficSurface>
              <div className="flex flex-wrap items-center justify-between gap-2 px-1">
                <h1 className="m-0! text-lg! font-medium">{title}</h1>
                {view !== 'overview' && report && !isError && (
                  <p className="text-on-surface-variant text-xs tabular-nums">
                    {usageAmount(report.total, 'bytes')} ·{' '}
                    {usageAmount(report.total, 'connections')}
                  </p>
                )}
              </div>
              {isError ? (
                <Notice error>{m.traffic_unavailable()}</Notice>
              ) : (
                <AnimatedOutletPreset className="relative flex min-w-0 flex-1 flex-col gap-3" />
              )}
            </TrafficSurface>
          </DataPage>
        </div>
      </SidebarProvider>
    </TrafficPageContext>
  )
}
