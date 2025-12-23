import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { useScrollArea } from '@/components/ui/scroll-area'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

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

  const isAllowControl = currentGroup?.type === 'select'

  const [lanes, setLanes] = useState(4)

  const { viewportRef } = useScrollArea()

  const virtualizer = useVirtualizer({
    count: currentGroup?.all?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    lanes,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = virtualizer.getVirtualItems()

  return (
    <div
      className="relative w-full"
      data-slot="proxies-virtual-list"
      style={{
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
            className="group absolute top-0 left-0 h-18 p-1"
            style={{
              transform: `translateY(${virtualItem.start}px)`,
              width: `${100 / lanes}%`,
              height: `${virtualItem.size}px`,
              left: `${virtualItem.lane * (100 / lanes)}%`,
            }}
            data-index={virtualItem.index}
            data-slot="proxies-virtual-item"
            data-active={String(proxy.name === currentGroup?.now)}
          >
            <Button
              variant="fab"
              className={cn(
                'w-full',
                'group-data-[active=true]:bg-surface-variant/80',
                'group-data-[active=false]:bg-transparent',
                'group-data-[active=false]:shadow-none',
                'group-data-[active=false]:hover:shadow-none',
                'group-data-[active=false]:hover:bg-surface-variant/30',
              )}
            >
              <div className="flex items-center gap-2">
                <div className="truncate text-sm font-medium">{proxy.name}</div>
              </div>
            </Button>
          </div>
        )
      })}
    </div>
  )
}
