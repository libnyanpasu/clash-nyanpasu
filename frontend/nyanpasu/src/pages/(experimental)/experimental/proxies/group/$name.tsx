import Radar from '~icons/material-symbols/radar'
import { useCallback } from 'react'
import { Button } from '@/components/ui/button'
import { useScrollArea } from '@/components/ui/scroll-area'
import {
  ClashProxiesQueryProxyItem,
  useClashProxies,
} from '@nyanpasu/interface'
import { useContainerBreakpointValue } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import GroupHeader from './_modules/group-header'
import ProxyNodeButton from './_modules/proxy-node-button'

export const Route = createFileRoute(
  '/(experimental)/experimental/proxies/group/$name',
)({
  component: RouteComponent,
})

function RouteComponent() {
  const { name: proxyGroupName } = Route.useParams()

  const { data: proxies } = useClashProxies()

  const currentGroup = proxies?.groups.find(
    (group) => group.name === proxyGroupName,
  )

  const { viewportRef } = useScrollArea()

  // define the number of lanes based on the container breakpoint
  const lanes = useContainerBreakpointValue(
    viewportRef,
    {
      xs: 2,
      sm: 3,
      md: 4,
      lg: 5,
      xl: 6,
    },
    4,
  )

  const virtualizer = useVirtualizer({
    count: currentGroup?.all?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    lanes,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = virtualizer.getVirtualItems()

  const handleScrollToCurrentNode = useCallback(() => {
    const index = currentGroup?.all?.findIndex(
      (proxy) => proxy.name === currentGroup?.now,
    )

    // unwarp undefined index
    if (index !== undefined) {
      virtualizer.scrollToIndex(index, {
        align: 'center',
        behavior: 'smooth',
      })
    }
  }, [currentGroup?.all, currentGroup?.now, virtualizer])

  return (
    <>
      <GroupHeader>
        <span>{currentGroup?.name}</span>

        <div className="flex-1" />

        <Button icon className="size-8" onClick={handleScrollToCurrentNode}>
          <Radar className="size-4" />
        </Button>
      </GroupHeader>

      <div
        className="relative m-2"
        data-slot="proxies-virtual-list"
        style={{
          width: 'calc(100% - 16px)',
          height: `${virtualizer.getTotalSize()}px`,
        }}
      >
        {virtualItems.map((virtualItem) => {
          const proxy = currentGroup?.all?.[virtualItem.index]

          if (!proxy) {
            return null
          }

          return (
            <div
              key={virtualItem.index}
              ref={virtualizer.measureElement}
              className="group absolute top-0 left-0 p-1"
              style={{
                transform: `translateY(${virtualItem.start}px)`,
                width: `${100 / lanes}%`,
                left: `${virtualItem.lane * (100 / lanes)}%`,
              }}
              data-index={virtualItem.index}
              data-slot="proxies-virtual-item"
              data-active={String(proxy.name === currentGroup?.now)}
            >
              <ProxyNodeButton proxy={proxy} />
            </div>
          )
        })}
      </div>
    </>
  )
}
