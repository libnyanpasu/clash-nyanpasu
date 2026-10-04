import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import {
  memo,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
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

const CoreRecord = memo(function CoreRecord({
  row,
  detail,
  expanded,
  onInspect,
  search,
}: {
  row: CoreLogRow
  detail: (cursor: CoreLogCursor) => Promise<CoreLogRecord>
  expanded: boolean
  onInspect: (key: string) => void
  search: string
}) {
  const loadRaw = useCallback(() => detail(row.id), [detail, row.id])
  const inspect = useCallback(
    () => onInspect(`${row.id.generation}:${row.id.sequence}`),
    [onInspect, row.id],
  )
  const timeTitle = useMemo(
    () => new Date(row.record.received_at).toLocaleString(),
    [row.record.received_at],
  )

  return (
    <LogRecord
      time={row.record.time || ''}
      timeTitle={timeTitle}
      level={row.record.type}
      message={row.record.payload}
      raw={row.record}
      loadRaw={loadRaw}
      expanded={expanded}
      incomplete={row.truncated}
      incompleteTitle={m.logs_core_preview_truncated()}
      search={search}
      onInspect={inspect}
    />
  )
})

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
  const { loadOlder } = logs
  const rows = useDeferredValue(logs.data)
  const deferredSearch = useDeferredValue(search)
  const [inspected, setInspected] = useState<string | null>(null)
  const inspect = useCallback(
    (key: string) => {
      setFollowing(false)
      setInspected((current) => (current === key ? null : key))
    },
    [setFollowing],
  )
  const { isBottom, scrollDirection, isTop } = useScrollArea()
  const { viewportRef } = useScrollAreaViewport()
  const anchor = useRef<{
    id: CoreLogCursor
    delta: number
    firstId: CoreLogCursor | undefined
  } | null>(null)
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
    const frame = requestAnimationFrame(() => {
      if (following) anchor.current = null
      if (anchor.current) {
        if (rows !== logs.data) return
        if (rows[0]?.id === anchor.current.firstId && logs.more) return
        const index = rows.findIndex(
          (row) =>
            row.id.generation === anchor.current?.id.generation &&
            row.id.sequence === anchor.current?.id.sequence,
        )
        if (index >= 0) {
          const offset = rowVirtualizer.getOffsetForIndex(index, 'start')
          if (offset)
            rowVirtualizer.scrollToOffset(offset[0] + anchor.current.delta)
        }
        anchor.current = null
      } else if (following && rows.length) {
        rowVirtualizer.scrollToIndex(rows.length - 1, { align: 'end' })
      }
    })
    return () => cancelAnimationFrame(frame)
  }, [rows, logs.data, logs.more, following, rowVirtualizer, totalSize])
  useEffect(() => {
    if (
      logs.isLoading ||
      logs.loadingOlder ||
      logs.error ||
      logs.status?.error ||
      !logs.more ||
      rows !== logs.data
    )
      return
    const viewport = viewportRef.current
    const fitsViewport =
      viewport && viewport.scrollHeight <= viewport.clientHeight
    if (
      rows.length &&
      !fitsViewport &&
      (following || !isTop || (viewport?.scrollTop ?? 0) > 0)
    )
      return

    // Continue short or empty filtered pages even without a scrollbar.
    const timer = setTimeout(() => {
      const first = rowVirtualizer
        .getVirtualItems()
        .find((item) => item.end > (viewportRef.current?.scrollTop ?? 0))
      const row = first && rows[first.index]
      if (row && first)
        anchor.current = {
          id: row.id,
          delta: (viewportRef.current?.scrollTop ?? 0) - first.start,
          firstId: rows[0]?.id,
        }
      setFollowing(false)
      loadOlder()
    }, 100)
    return () => clearTimeout(timer)
  }, [
    logs.isLoading,
    logs.loadingOlder,
    logs.error,
    logs.status?.error,
    logs.more,
    logs.data,
    loadOlder,
    rows,
    following,
    isTop,
    rowVirtualizer,
    viewportRef,
    setFollowing,
  ])

  return (
    <div data-slot="core-logs-viewer">
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
                  onInspect={inspect}
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
  useEffect(() => setFollowing(true), [level, debounced])

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
          Boolean(logs.status?.discarded)) && (
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
                key={`${level}:${debounced}`}
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
