import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import { useCallback, useDeferredValue, useEffect, useState } from 'react'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import {
  ScrollArea,
  useScrollArea,
  useScrollAreaViewport,
} from '@nyanpasu/ui/scroll-area'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashLogs } from '@nyanpasu/query'
import type {
  CoreLogCursor,
  CoreLogRecord,
  CoreLogRow,
} from '@nyanpasu/rpc/types'
import { Button } from '@nyanpasu/ui'
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
type CoreLogs = ReturnType<typeof useClashLogs>

function CoreRecord({
  row,
  detail,
  expanded,
  onInspect,
  search,
}: {
  row: CoreLogRow
  detail: (cursor: CoreLogCursor) => Promise<CoreLogRecord>
  expanded: boolean
  onInspect: () => void
  search: string
}) {
  const loadRaw = useCallback(() => detail(row.id), [detail, row.id])

  return (
    <LogRecord
      time={row.record.time || ''}
      timeTitle={new Date(row.record.received_at).toLocaleString()}
      level={row.record.type}
      message={row.record.payload}
      raw={row.record}
      loadRaw={loadRaw}
      expanded={expanded}
      incomplete={row.truncated}
      incompleteTitle={m.logs_core_preview_truncated()}
      search={search}
      onInspect={onInspect}
    />
  )
}

function Viewer({
  logs,
  search,
  following,
  setFollowing,
}: {
  logs: CoreLogs
  search: string
  following: boolean
  setFollowing: (value: boolean) => void
}) {
  const rows = useDeferredValue(logs.data)
  const deferredSearch = useDeferredValue(search)
  const [inspected, setInspected] = useState<string | null>(null)
  const { isBottom, scrollDirection } = useScrollArea()
  const { viewportRef } = useScrollAreaViewport()
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewportRef.current,
    getItemKey: (index) => {
      const id = rows[index].id
      return `${id.generation}:${id.sequence}`
    },
    estimateSize: () => 110,
    overscan: 5,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
  })
  const totalSize = rowVirtualizer.getTotalSize()
  const showRows = useDeferredValue(true, false)

  useEffect(() => {
    rowVirtualizer.measure()
  }, [rowVirtualizer])
  useEffect(() => {
    if (scrollDirection === 'up' && !isBottom) setFollowing(false)
  }, [scrollDirection, isBottom, setFollowing])
  useEffect(() => {
    if (!following || !rows.length) return
    const frame = requestAnimationFrame(() =>
      rowVirtualizer.scrollToIndex(rows.length - 1, { align: 'end' }),
    )
    return () => cancelAnimationFrame(frame)
  }, [rows, following, rowVirtualizer, totalSize])

  return (
    <div data-slot="core-logs-viewer">
      {logs.more && (
        <Button
          disabled={logs.loadingOlder}
          onClick={() => {
            setFollowing(false)
            logs.loadOlder()
          }}
        >
          {m.logs_load_older()}
        </Button>
      )}
      {!rows.length && !logs.isLoading && (
        <LogEmptyState>
          {logs.more ? m.logs_search_incomplete() : m.logs_empty_message()}
        </LogEmptyState>
      )}
      <div
        className="relative"
        data-slot="logs-virtual-list"
        style={{ height: totalSize }}
      >
        {showRows &&
          rowVirtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index]
            if (!row) return null
            const key = String(item.key)
            return (
              <div
                key={item.key}
                ref={rowVirtualizer.measureElement}
                data-index={item.index}
                data-slot="logs-virtual-item"
                className="absolute top-0 left-0 w-full select-text"
                style={{ transform: `translateY(${item.start}px)` }}
              >
                <CoreRecord
                  row={row}
                  detail={logs.detail}
                  expanded={inspected === key}
                  search={deferredSearch}
                  onInspect={() => {
                    setFollowing(false)
                    setInspected((current) => (current === key ? null : key))
                  }}
                />
              </div>
            )
          })}
      </div>
    </div>
  )
}

function KernelLogs() {
  const { level } = IndexRoute.useSearch()
  const [following, setFollowing] = useState(true)
  const [search, setSearch] = useState('')
  const [debounced, setDebounced] = useState(search)
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(search), 250)
    return () => clearTimeout(timer)
  }, [search])

  const logs = useClashLogs(level ?? null, debounced, following)
  const clear = useLockFn(async () => {
    await logs.clean.mutateAsync()
  })
  const canClear = Boolean(logs.status?.head)
  const error = logs.error || logs.status?.error

  return (
    <LogsLayout
      source="core"
      search={
        <LogSearch
          value={search}
          onChange={setSearch}
          placeholder={m.logs_search_placeholder()}
        />
      }
      actions={
        <LogClearButton
          disabled={!canClear}
          onClick={clear}
          label={m.logs_core_clear_history()}
        />
      }
    >
      <div className={logPanelClass} data-slot="core-logs">
        {(logs.isLoading ||
          logs.loadingOlder ||
          error ||
          logs.status?.discarded) && (
          <div
            className="text-on-surface-variant flex flex-wrap items-center gap-2 px-4 py-2 text-xs"
            role="status"
          >
            {logs.isLoading && <span>{m.logs_loading()}</span>}
            {logs.loadingOlder && <span>{m.logs_load_older()}…</span>}
            {error && (
              <>
                <span
                  className="text-error"
                  title={
                    typeof error === 'string'
                      ? error
                      : error.kind === 'unavailable'
                        ? error.message
                        : error.kind
                  }
                >
                  {m.logs_source_unavailable()}
                </span>
                <Button onClick={() => logs.retry()}>{m.logs_retry()}</Button>
              </>
            )}
            {Boolean(logs.status?.discarded) && (
              <span>
                {m.logs_core_discarded({
                  count: String(logs.status?.discarded),
                })}
              </span>
            )}
          </div>
        )}
        <RegisterContextMenu>
          <RegisterContextMenuTrigger asChild>
            <ScrollArea className="min-h-0 flex-1">
              <Viewer
                logs={logs}
                search={search}
                following={following}
                setFollowing={setFollowing}
              />
            </ScrollArea>
          </RegisterContextMenuTrigger>
          <RegisterContextMenuContent>
            <ContextMenuItem disabled={!canClear} onClick={clear}>
              <DeleteForeverOutlineRounded className="size-4" />
              <span>{m.logs_core_clear_history()}</span>
            </ContextMenuItem>
          </RegisterContextMenuContent>
        </RegisterContextMenu>
        {!following && <LogFollowButton onClick={() => setFollowing(true)} />}
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
