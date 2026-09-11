import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import { useEffect, useMemo, useState } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { useClashLogs } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import FileLogs from './_modules/file-logs'
import {
  LogClearButton,
  LogEmptyState,
  LogFollowButton,
  logPanelClass,
  LogRecord,
  LogSearch,
} from './_modules/log-viewer'
import { LogsLayout } from './_modules/logs-layout'
import { Route as IndexRoute } from './route'

export const Route = createFileRoute('/(main)/main/logs/')({
  component: RouteComponent,
})

const Viewer = ({
  search,
  following,
  setFollowing,
}: {
  search: string
  following: boolean
  setFollowing: (value: boolean) => void
}) => {
  const { level } = IndexRoute.useSearch()
  const { data: logs } = useClashLogs()
  const filteredLogs = useMemo(() => {
    if (!logs) return []
    if (!level) return logs
    return logs.filter(
      (log) =>
        log.type.toLowerCase().replace('warning', 'warn') ===
        level.replace('warning', 'warn'),
    )
  }, [logs, level])
  const { isBottom, viewportRef, scrollDirection } = useScrollArea()
  const rowVirtualizer = useVirtualizer({
    count: filteredLogs.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 110,
    overscan: 5,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
  })
  const totalSize = rowVirtualizer.getTotalSize()
  useEffect(() => {
    // ScrollArea attaches its viewport ref after this child's layout effects.
    rowVirtualizer.measure()
  }, [rowVirtualizer])
  useEffect(() => {
    if (scrollDirection === 'up' && !isBottom) setFollowing(false)
  }, [scrollDirection, isBottom, setFollowing])
  useEffect(() => {
    if (!following || !filteredLogs.length) return
    const frame = requestAnimationFrame(() =>
      rowVirtualizer.scrollToIndex(filteredLogs.length - 1, { align: 'end' }),
    )
    return () => cancelAnimationFrame(frame)
  }, [filteredLogs, following, rowVirtualizer, totalSize])
  return (
    <div>
      {!filteredLogs.length && <LogEmptyState />}
      <div
        className="relative"
        data-slot="logs-virtual-list"
        style={{ height: totalSize }}
      >
        {rowVirtualizer.getVirtualItems().map((item) => {
          const log = filteredLogs[item.index]
          if (!log) return null
          return (
            <div
              key={item.key}
              ref={rowVirtualizer.measureElement}
              data-index={item.index}
              data-slot="logs-virtual-item"
              className="absolute top-0 left-0 w-full select-text"
              style={{
                transform: `translateY(${item.start}px)`,
              }}
            >
              <LogRecord
                time={log.time || ''}
                level={log.type}
                message={log.payload || ''}
                raw={JSON.stringify(log, null, 2)}
                search={search}
                onInspect={() => setFollowing(false)}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}

function KernelLogs() {
  const [following, setFollowing] = useState(true)
  const [search, setSearch] = useState('')
  const { data: logs, clean } = useClashLogs()
  const handleClearLogs = useLockFn(async () => {
    await clean.mutateAsync()
  })
  return (
    <LogsLayout
      source="core"
      actions={
        <LogClearButton disabled={!logs?.length} onClick={handleClearLogs} />
      }
    >
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <LogSearch
          value={search}
          onChange={setSearch}
          placeholder={m.logs_search_placeholder()}
        />
        <div className={logPanelClass}>
          <RegisterContextMenu>
            <RegisterContextMenuTrigger asChild>
              <ScrollArea className="min-h-0 flex-1">
                <Viewer
                  search={search}
                  following={following}
                  setFollowing={setFollowing}
                />
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
          {!following && <LogFollowButton onClick={() => setFollowing(true)} />}
        </div>
      </div>
    </LogsLayout>
  )
}

function RouteComponent() {
  const { source = 'core' } = IndexRoute.useSearch()
  return source === 'core' ? (
    <KernelLogs />
  ) : (
    <FileLogs key={source} source={source} />
  )
}
