import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import HighlightText from '@/components/ui/highlight-text'
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area'
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
import LogLevelBadge from './log-level-badge'

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
const inputClass = 'bg-surface-variant/40 h-9 min-w-0 rounded-lg px-3 text-sm'

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
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-slot="file-logs">
      <div className="border-outline-variant grid grid-cols-1 gap-2 border-b p-3 sm:grid-cols-2 xl:grid-cols-4">
        <label className="flex min-w-0 flex-col gap-1 text-xs">
          {m.logs_file_label()}
          <select
            className={inputClass}
            value={file ?? ''}
            onChange={(event) => setFile(event.target.value || null)}
          >
            <option value="">{m.logs_current_file()}</option>
            {logs.files.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex min-w-0 flex-col gap-1 text-xs">
          {m.logs_target_label()}
          <input
            className={inputClass}
            value={target}
            maxLength={4096}
            onChange={(event) => setTarget(event.target.value)}
          />
        </label>
        <label className="flex min-w-0 flex-col gap-1 text-xs">
          {m.logs_from_label()}
          <input
            className={inputClass}
            type="datetime-local"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
          />
        </label>
        <label className="flex min-w-0 flex-col gap-1 text-xs">
          {m.logs_to_label()}
          <input
            className={inputClass}
            type="datetime-local"
            value={to}
            onChange={(event) => setTo(event.target.value)}
          />
        </label>
      </div>
      <div
        className="flex flex-wrap items-center gap-2 px-3 py-2 text-xs"
        role="status"
      >
        {logs.loading && (
          <span>
            {m.logs_loading()}{' '}
            {logs.page &&
              `${(Number(logs.page.indexed_bytes) / 1048576).toFixed(1)} / ${(Number(logs.page.file_bytes) / 1048576).toFixed(1)} MiB`}
          </span>
        )}
        {logs.error && (
          <>
            <span>{errorMessage(logs.error)}</span>
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
        <Button
          className="ml-auto"
          disabled={logs.loading || !logs.rows.length}
          onClick={() => logs.clear()}
        >
          {m.logs_clear_display()}
        </Button>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <FileRows logs={logs} search={debounced.search} />
      </ScrollArea>
      <div className="border-outline-variant border-t p-3">
        <input
          className={`${inputClass} w-full`}
          aria-label={m.logs_filter_placeholder()}
          placeholder={m.logs_filter_placeholder()}
          value={search}
          maxLength={256}
          onChange={(event) => setSearch(event.target.value)}
        />
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
  const [copyError, setCopyError] = useState(false)
  const lastId = useRef<string | undefined>(undefined)
  const anchor = useRef<{
    id: string
    delta: number
    firstId: string | undefined
  } | null>(null)
  const header = useRef<HTMLDivElement>(null)
  const [headerHeight, setHeaderHeight] = useState(56)
  useLayoutEffect(() => {
    if (!header.current) return
    let frame = 0
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() =>
        setHeaderHeight(header.current?.offsetHeight ?? 56),
      )
    })
    observer.observe(header.current)
    return () => {
      observer.disconnect()
      cancelAnimationFrame(frame)
    }
  }, [])
  const virtualizer = useVirtualizer({
    count: logs.rows.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 76,
    overscan: 5,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
    scrollMargin: headerHeight,
    getItemKey: (index) => logs.rows[index]?.id ?? index,
  })
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
  }, [logs.rows, logs.more, following, virtualizer])
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
    <div className="px-4 pb-4">
      <div
        ref={header}
        className="bg-mixed-background sticky top-0 z-10 flex flex-wrap items-center gap-2 py-2"
      >
        <Button disabled={!logs.more || logs.loading} onClick={loadOlder}>
          {m.logs_load_older()}
        </Button>
        <Button
          aria-pressed={following}
          onClick={() => {
            anchor.current = null
            setFollowing(true)
            logs.latest()
          }}
        >
          {m.logs_follow_latest()}
          {unseen > 0 && ` (${unseen})`}
        </Button>
        {copyError && <span role="alert">{m.logs_copy_failed()}</span>}
      </div>
      {!logs.rows.length && !logs.loading && !logs.error && (
        <p className="py-12 text-center text-sm">
          {logs.more ? m.logs_search_incomplete() : m.logs_empty_message()}
        </p>
      )}
      <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((item) => {
          const row = logs.rows[item.index]
          if (!row) return null
          return (
            <div
              key={row.id}
              ref={virtualizer.measureElement}
              data-index={item.index}
              className="border-outline-variant absolute top-0 left-0 w-full border-b py-2 font-mono text-sm break-words"
              style={{
                transform: `translateY(${item.start - headerHeight}px)`,
              }}
            >
              <div className="flex flex-wrap items-center gap-2">
                <time>
                  {row.timestamp
                    ? new Date(Number(row.timestamp)).toLocaleString()
                    : '—'}
                </time>
                <LogLevelBadge>{row.level}</LogLevelBadge>
                <span className="min-w-0 break-all">
                  <HighlightText searchText={search}>
                    {row.target}
                  </HighlightText>
                </span>
                <button
                  className="text-primary ml-auto cursor-pointer text-xs"
                  onClick={() => {
                    navigator.clipboard.writeText(row.raw).then(
                      () => setCopyError(false),
                      () => setCopyError(true),
                    )
                  }}
                >
                  {m.logs_copy()}
                </button>
              </div>
              <div className="break-all whitespace-pre-wrap">
                <HighlightText searchText={search}>{row.message}</HighlightText>
              </div>
              <details className="mt-1 text-xs">
                <summary className="cursor-pointer">
                  {m.logs_raw_record()}
                  {row.unparsed || row.truncated ? ' ⚠' : ''}
                </summary>
                <pre className="mt-2 break-all whitespace-pre-wrap">
                  {row.raw}
                </pre>
              </details>
            </div>
          )
        })}
      </div>
    </div>
  )
}
