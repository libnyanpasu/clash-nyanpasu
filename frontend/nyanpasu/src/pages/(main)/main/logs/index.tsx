import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import { useEffect, useMemo, useState } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { ContextMenuItem } from '@/components/ui/context-menu'
import HighlightText from '@/components/ui/highlight-text'
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { useClashLogs } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import LogLevelBadge from './_modules/log-level-badge'
import { Route as IndexRoute } from './route'

export const Route = createFileRoute('/(main)/main/logs/')({
  component: RouteComponent,
})

const Viewer = ({ search }: { search: string }) => {
  const { level } = IndexRoute.useSearch()

  const {
    query: { data: logs },
  } = useClashLogs()

  const filteredLogs = useMemo(() => {
    if (!logs) {
      return []
    }

    if (!level) {
      return logs
    }

    return logs.filter((log) => log.type.toLowerCase() === level)
  }, [logs, level])

  const { isBottom, viewportRef } = useScrollArea()

  const rowVirtualizer = useVirtualizer({
    count: filteredLogs.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = rowVirtualizer.getVirtualItems()

  useEffect(() => {
    if (isBottom && filteredLogs.length > 0) {
      rowVirtualizer.scrollToIndex(filteredLogs.length - 1, {
        align: 'end',
        behavior: 'smooth',
      })
    }
  }, [filteredLogs, isBottom, rowVirtualizer])

  if (filteredLogs.length === 0) {
    return (
      <div
        className="absolute inset-0 flex flex-col items-center justify-center gap-4"
        data-slot="logs-no-logs"
      >
        <BoxOutlineRounded className="text-surface-variant size-16" />

        <p
          className="text-surface-variant text-sm"
          data-slot="logs-no-logs-message"
        >
          {m.logs_empty_message()}
        </p>
      </div>
    )
  }

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
        const log = filteredLogs[virtualItem.index]

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
  )
}

function RouteComponent() {
  const [search, setSearch] = useState('')

  const {
    query: { data: logs },
    clean,
  } = useClashLogs()

  const handleClearLogs = useLockFn(async () => {
    await clean.mutateAsync()
  })

  return (
    <div className="divide-outline-variant flex h-full min-h-0 flex-1 flex-col divide-y overflow-hidden">
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <ScrollArea className="min-h-0 flex-1">
            <Viewer search={search} />
          </ScrollArea>
        </RegisterContextMenuTrigger>

        <RegisterContextMenuContent>
          <ContextMenuItem
            disabled={logs?.length === 0}
            onClick={handleClearLogs}
          >
            <DeleteForeverOutlineRounded className="size-4" />
            <span>{m.logs_action_clear_log()}</span>
          </ContextMenuItem>
        </RegisterContextMenuContent>
      </RegisterContextMenu>

      <div
        className="bg-mixed-background flex h-16 shrink-0 items-center px-4"
        data-slot="logs-search"
      >
        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 w-full rounded-full px-4 pr-10 text-sm outline-none',
          )}
          data-slot="logs-search-input-field"
          placeholder={m.logs_search_placeholder()}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
    </div>
  )
}
