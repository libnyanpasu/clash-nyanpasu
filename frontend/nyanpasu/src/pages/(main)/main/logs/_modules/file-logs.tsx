import ExpandMoreRounded from '~icons/material-symbols/expand-more-rounded'
import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import HistoryRounded from '~icons/material-symbols/history-rounded'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { m } from '@/paraglide/messages'
import {
  useFileLogs,
  type Filter,
  type Level,
  type LogError,
  type LogSource,
} from '@nyanpasu/interface'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Route } from '../route'
import {
  LogEmptyState,
  logPanelClass,
  LogRecord,
  LogSearch,
  LogViewerActions,
  useLogToolbar,
} from './log-viewer'

function errorMessage(error: LogError) {
  switch (error) {
    case 'unsupported':
      return m.logs_service_unsupported()
    case 'file_gone':
      return m.logs_file_gone()
    case 'limit':
      return m.logs_resource_limit()
    case 'invalid_request':
      return m.logs_invalid_filter()
    default:
      return m.logs_source_unavailable()
  }
}
const inputClass =
  'bg-surface-variant/30 text-on-surface border-outline-variant focus:border-primary focus:ring-primary h-11 min-w-0 w-full rounded-xl border px-3 text-sm outline-none focus:ring-1 [color-scheme:light] dark:[color-scheme:dark]'

export default function FileLogs({ source }: { source: LogSource }) {
  const { level } = Route.useSearch()
  const [file, setFile] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [target, setTarget] = useState('')
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const [debounced, setDebounced] = useState({ search, target })
  useEffect(() => {
    const timer = setTimeout(() => setDebounced({ search, target }), 250)
    return () => clearTimeout(timer)
  }, [search, target])
  const filter = useMemo<Filter>(
    () => ({
      levels: level ? [level === 'warning' ? 'warn' : (level as Level)] : [],
      target: debounced.target || null,
      text: debounced.search || null,
      from_ms: from ? Date.parse(from) : null,
      to_ms: to ? Date.parse(to) : null,
    }),
    [level, debounced, from, to],
  )
  const logs = useFileLogs(source, file, filter)
  const filterCount = [file, target, from, to].filter(Boolean).length
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-slot="file-logs">
      <LogSearch
        value={search}
        onChange={setSearch}
        placeholder={m.logs_filter_placeholder()}
      />
      <details className="group bg-surface-variant/20 mx-3 mb-3 shrink-0 rounded-2xl">
        <summary className="text-on-surface-variant focus-visible:outline-primary flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-2xl px-4 text-sm font-medium focus-visible:outline-2 [&::-webkit-details-marker]:hidden">
          <FilterListRounded aria-hidden className="size-5" />
          {m.logs_filters()}
          {filterCount > 0 && (
            <span className="bg-secondary-container text-on-secondary-container rounded-full px-2 text-xs tabular-nums">
              {filterCount}
            </span>
          )}
          <ExpandMoreRounded
            aria-hidden
            className="ml-auto size-5 transition-transform group-open:rotate-180"
          />
        </summary>
        <div className="grid max-h-[40vh] grid-cols-1 items-end gap-3 overflow-y-auto px-4 pb-4 sm:grid-cols-2 xl:grid-cols-4">
          <div className="min-w-0">
            <Select
              variant="outlined"
              value={file ?? 'current'}
              onValueChange={(value) =>
                setFile(value === 'current' ? null : value)
              }
            >
              <SelectTrigger
                className="min-w-0"
                aria-label={m.logs_file_label()}
              >
                <SelectValue
                  className="truncate pr-4 text-sm"
                  placeholder={m.logs_file_label()}
                >
                  {file
                    ? (logs.files.find((entry) => entry.id === file)?.name ??
                      file)
                    : m.logs_current_file()}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="current">{m.logs_current_file()}</SelectItem>
                {logs.files.map((entry) => (
                  <SelectItem key={entry.id} value={entry.id}>
                    {entry.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="min-w-0">
            <Input
              variant="outlined"
              label={m.logs_target_label()}
              aria-label={m.logs_target_label()}
              className="text-sm"
              value={target}
              maxLength={4096}
              onChange={(event) => setTarget(event.target.value)}
            />
          </div>
          <label className="text-on-surface-variant flex min-w-0 flex-col gap-1 text-xs">
            {m.logs_from_label()}
            <input
              className={inputClass}
              type="datetime-local"
              value={from}
              onChange={(event) => setFrom(event.target.value)}
            />
          </label>
          <label className="text-on-surface-variant flex min-w-0 flex-col gap-1 text-xs">
            {m.logs_to_label()}
            <input
              className={inputClass}
              type="datetime-local"
              value={to}
              onChange={(event) => setTo(event.target.value)}
            />
          </label>
        </div>
      </details>
      <div className={logPanelClass}>
        {(logs.loading || logs.error || logs.page) && (
          <div
            className="text-on-surface-variant flex flex-wrap items-center gap-2 px-4 py-2 text-xs"
            role="status"
          >
            {logs.loading && (
              <span>
                {m.logs_loading()}
                {logs.page &&
                  ` ${(Number(logs.page.indexed_bytes) / 1048576).toFixed(1)} / ${(Number(logs.page.file_bytes) / 1048576).toFixed(1)} MiB`}
              </span>
            )}
            {logs.error && (
              <>
                <span className="text-error">{errorMessage(logs.error)}</span>
                <Button onClick={() => logs.retry()}>{m.logs_retry()}</Button>
              </>
            )}
            {logs.page?.partial && <span>{m.logs_partial_coverage()}</span>}
            {logs.page &&
              (logs.page.malformed !== '0' || logs.page.truncated !== '0') && (
                <span>
                  {m.logs_parse_diagnostics({
                    malformed: logs.page.malformed,
                    truncated: logs.page.truncated,
                  })}
                </span>
              )}
            {logs.page?.file && (
              <span className="min-w-0 truncate" title={logs.page.file}>
                {logs.page.file}
              </span>
            )}
          </div>
        )}
        <ScrollArea className="min-h-0 flex-1">
          <FileRows logs={logs} search={debounced.search} />
        </ScrollArea>
      </div>
    </div>
  )
}

function FileRows({
  logs,
  search,
}: {
  logs: ReturnType<typeof useFileLogs>
  search: string
}) {
  const { viewportRef, isBottom, scrollDirection } = useScrollArea()
  const [following, setFollowing] = useState(true)
  const [unseen, setUnseen] = useState(0)
  const lastId = useRef<string | undefined>(undefined)
  const anchor = useRef<{
    id: string
    delta: number
    firstId: string | undefined
  } | null>(null)
  const toolbar = useLogToolbar()
  const virtualizer = useVirtualizer({
    count: logs.rows.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 110,
    overscan: 5,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
    scrollMargin: toolbar.height,
    getItemKey: (index) => logs.rows[index]?.id ?? index,
  })
  const totalSize = virtualizer.getTotalSize()
  useEffect(() => {
    if (scrollDirection === 'up' && !isBottom) setFollowing(false)
  }, [scrollDirection, isBottom])
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      if (anchor.current) {
        if (logs.rows[0]?.id === anchor.current.firstId && logs.more) return
        const index = logs.rows.findIndex(
          (row) => row.id === anchor.current?.id,
        )
        if (index >= 0) {
          const offset = virtualizer.getOffsetForIndex(index, 'start')
          if (offset)
            virtualizer.scrollToOffset(offset[0] + anchor.current.delta)
        }
        anchor.current = null
      } else if (following && logs.rows.length) {
        virtualizer.scrollToIndex(logs.rows.length - 1, { align: 'end' })
      }
    })
    return () => cancelAnimationFrame(frame)
  }, [logs.rows, logs.more, following, virtualizer, totalSize, toolbar.height])
  useEffect(() => {
    const latest = logs.rows.at(-1)?.id
    if (!following && latest && lastId.current && latest !== lastId.current) {
      const previous = logs.rows.findIndex((row) => row.id === lastId.current)
      setUnseen(
        (count) => count + (previous < 0 ? 1 : logs.rows.length - previous - 1),
      )
    }
    lastId.current = latest
    if (following) setUnseen(0)
  }, [logs.rows, following])
  const loadOlder = () => {
    const first = virtualizer
      .getVirtualItems()
      .find((item) => item.end > (viewportRef.current?.scrollTop ?? 0))
    const row = first && logs.rows[first.index]
    if (row && first)
      anchor.current = {
        id: row.id,
        delta: (viewportRef.current?.scrollTop ?? 0) - first.start,
        firstId: logs.rows[0]?.id,
      }
    setFollowing(false)
    logs.loadOlder()
  }
  return (
    <div>
      <LogViewerActions
        ref={toolbar.ref}
        following={following}
        unseen={unseen}
        clearDisabled={logs.loading || !logs.rows.length}
        onClear={() => logs.clear()}
        onFollow={() => {
          anchor.current = null
          setFollowing(true)
          logs.latest()
        }}
      >
        <Button
          className="focus-visible:ring-primary flex w-10 min-w-10 items-center justify-center gap-2 p-0 focus-visible:ring-2 md:w-auto md:px-4"
          disabled={!logs.more || logs.loading}
          aria-disabled={!logs.more || logs.loading}
          aria-label={m.logs_load_older()}
          title={m.logs_load_older()}
          onClick={loadOlder}
        >
          <HistoryRounded aria-hidden className="size-5" />
          <span className="hidden md:inline">{m.logs_load_older()}</span>
        </Button>
      </LogViewerActions>
      {!logs.rows.length && !logs.loading && !logs.error && (
        <LogEmptyState>
          {logs.more ? m.logs_search_incomplete() : m.logs_empty_message()}
        </LogEmptyState>
      )}
      <div className="relative" style={{ height: totalSize }}>
        {virtualizer.getVirtualItems().map((item) => {
          const row = logs.rows[item.index]
          if (!row) return null
          const date = row.timestamp ? new Date(Number(row.timestamp)) : null
          return (
            <div
              key={row.id}
              ref={virtualizer.measureElement}
              data-index={item.index}
              className="absolute top-0 left-0 w-full select-text"
              style={{
                transform: `translateY(${item.start - toolbar.height}px)`,
              }}
            >
              <LogRecord
                time={date?.toLocaleString() ?? '—'}
                timeTitle={date?.toLocaleString()}
                level={row.level}
                target={row.target}
                message={row.message}
                raw={row.raw}
                search={search}
                incomplete={row.unparsed || row.truncated}
                onInspect={() => setFollowing(false)}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}
