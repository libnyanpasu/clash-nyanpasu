import { useEffect } from 'react'
import {
  AppContentScrollArea,
  useScrollArea,
} from '@/components/ui/scroll-area'
import { useClashLogs } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import LogLevelBadge from './_modules/log-level-badge'

export const Route = createFileRoute('/(experimental)/experimental/logs')({
  component: RouteComponent,
})

const InnerComponent = () => {
  const {
    query: { data: logs },
  } = useClashLogs()

  const { isBottom, viewportRef } = useScrollArea()

  const rowVirtualizer = useVirtualizer({
    count: logs?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = rowVirtualizer.getVirtualItems()

  useEffect(() => {
    if (isBottom && logs && logs.length > 0) {
      rowVirtualizer.scrollToIndex(logs.length - 1, {
        align: 'end',
        behavior: 'smooth',
      })
    }
  }, [logs, isBottom, rowVirtualizer])

  return (
    <div
      className={cn(
        'relative mx-4 flex flex-col',
        'divide-outline-variant divide-y',
      )}
      data-slot="logs-virtual-list"
      style={{
        height: `${rowVirtualizer.getTotalSize()}px`,
      }}
    >
      {virtualItems.map((virtualItem) => {
        const log = logs?.[virtualItem.index]

        if (!log) {
          return null
        }

        return (
          <div
            key={virtualItem.key}
            ref={rowVirtualizer.measureElement}
            className={cn(
              'absolute top-0 left-0 w-full select-text',
              'font-mono break-all',
              'flex flex-col py-2',
            )}
            data-index={virtualItem.index}
            data-slot="logs-virtual-item"
            style={{
              transform: `translateY(${virtualItem.start}px)`,
            }}
          >
            <div className="flex items-center gap-1">
              <span className="font-semibold">{log.time}</span>
              <LogLevelBadge>{log.type}</LogLevelBadge>
            </div>

            <div className="font-normal text-wrap">{log.payload}</div>
          </div>
        )
      })}
    </div>
  )
}

function RouteComponent() {
  return (
    <AppContentScrollArea>
      <InnerComponent />
    </AppContentScrollArea>
  )
}
