import { useEffect, useState } from 'react'
import HighlightText from '@/components/ui/highlight-text'
import {
  AppContentScrollArea,
  useScrollArea,
} from '@/components/ui/scroll-area'
import { useClashLogs } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import LogLevelBadge from './_modules/log-level-badge'

export const Route = createFileRoute('/(main)/main/logs')({
  component: RouteComponent,
})

const InnerComponent = () => {
  const {
    query: { data: logs },
  } = useClashLogs()

  const { isBottom, viewportRef } = useScrollArea()

  const [search, setSearch] = useState('')

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
    <>
      <div
        className="sticky top-0 z-10 px-4 py-4 backdrop-blur-xl"
        data-slot="logs-search"
      >
        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 w-full rounded-full px-4 pr-10 text-sm outline-none',
          )}
          data-slot="logs-search-input-field"
          placeholder="Search logs (time, type, or message)..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

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
              data-index={virtualItem.index}
              data-slot="logs-virtual-item"
              className={cn(
                'absolute top-0 left-0 w-full select-text',
                'font-mono break-all',
                'flex flex-col py-2',
                'data-[index=0]:pt-0',
              )}
              style={{
                transform: `translateY(${virtualItem.start}px)`,
              }}
            >
              <div className="flex items-center gap-1">
                <HighlightText searchText={search}>
                  {log.time || ''}
                </HighlightText>

                <LogLevelBadge searchText={search}>{log.type}</LogLevelBadge>
              </div>

              <div className="font-normal text-wrap">
                <HighlightText searchText={search}>
                  {log.payload || ''}
                </HighlightText>
              </div>
            </div>
          )
        })}
      </div>
    </>
  )
}

function RouteComponent() {
  return (
    <AppContentScrollArea>
      <InnerComponent />
    </AppContentScrollArea>
  )
}
