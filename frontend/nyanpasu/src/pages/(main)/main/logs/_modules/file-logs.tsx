import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
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
  LogClearButton,
  LogEmptyState,
  LogFollowButton,
  logPanelClass,
  LogRecord,
  LogSearch,
} from './log-viewer'
import { LogsLayout } from './logs-layout'

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
const DateTimeField = lazy(() => import('@/components/ui/date-time-field'))

export default function FileLogs({ source }: { source: LogSource }) {
  const { level } = Route.useSearch()
  const filterForm = useRef<HTMLFormElement>(null)
  const [following, setFollowing] = useState(true)
  const [unseen, setUnseen] = useState(0)
  const [filterOpen, setFilterOpen] = useState(false)
  const [draft, setDraft] = useState({ target: '', from: '', to: '' })
  const [file, setFile] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [target, setTarget] = useState('')
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const [debounced, setDebounced] = useState(search)
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(search), 250)
    return () => clearTimeout(timer)
  }, [search])
  const filter = useMemo<Filter>(
    () => ({
      levels: level ? [level === 'warning' ? 'warn' : (level as Level)] : [],
      target: target || null,
      text: debounced || null,
      from_ms: from ? Date.parse(from) : null,
      to_ms: to ? Date.parse(to) : null,
    }),
    [level, debounced, target, from, to],
  )
  useEffect(() => setFollowing(true), [file, filter])
  const logs = useFileLogs(source, file, filter)
  const filterCount = [target, from, to].filter(Boolean).length
  return (
    <LogsLayout
      source={source}
      actions={
        <>
          <div className="max-w-72 min-w-0 flex-1">
            <Select
              variant="outlined"
              value={file ?? 'current'}
              onValueChange={(value) => {
                setFollowing(true)
                setFile(value === 'current' ? null : value)
              }}
            >
              <SelectTrigger
                className="h-10 min-w-0 py-2"
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
          <Modal
            open={filterOpen}
            onOpenChange={(open) => {
              if (open) setDraft({ target, from, to })
              setFilterOpen(open)
            }}
          >
            <ModalTrigger asChild>
              <Button
                icon
                aria-label={m.logs_filters()}
                title={m.logs_filters()}
                className="focus-visible:ring-primary shrink-0 focus-visible:ring-2"
              >
                <FilterListRounded aria-hidden className="size-5" />
                {filterCount > 0 && (
                  <span className="bg-secondary-container text-on-secondary-container absolute top-0 right-0 rounded-full px-1 text-[10px]">
                    {filterCount}
                  </span>
                )}
              </Button>
            </ModalTrigger>
            <ModalContent
              onEscapeKeyDown={(event) => {
                // The calendar portal stays inside this dialog's focus scope.
                // Let its own Escape handler close it before dismissing the filters.
                if (
                  filterForm.current?.querySelector(
                    '[data-slot="date-picker-popover"]',
                  )
                )
                  event.preventDefault()
              }}
            >
              <form
                ref={filterForm}
                className="bg-surface text-on-surface flex max-h-[85dvh] w-[min(28rem,calc(100vw-2rem))] flex-col gap-5 overflow-auto rounded-3xl p-6"
                onSubmit={(event) => {
                  event.preventDefault()
                  setTarget(draft.target)
                  setFrom(draft.from)
                  setTo(draft.to)
                  setFollowing(true)
                  setFilterOpen(false)
                }}
              >
                <ModalTitle className="text-xl">{m.logs_filters()}</ModalTitle>
                <div className="grid gap-4">
                  <div className="min-w-0">
                    <Input
                      variant="outlined"
                      label={m.logs_target_label()}
                      aria-label={m.logs_target_label()}
                      className="text-sm"
                      value={draft.target}
                      maxLength={4096}
                      onChange={(event) =>
                        setDraft({ ...draft, target: event.target.value })
                      }
                    />
                  </div>
                  <Suspense
                    fallback={
                      <div
                        className="bg-surface-variant/30 h-40 animate-pulse rounded-xl"
                        aria-busy="true"
                      />
                    }
                  >
                    <DateTimeField
                      label={m.logs_from_label()}
                      value={draft.from}
                      onChange={(from) =>
                        setDraft((value) => ({ ...value, from }))
                      }
                    />
                    <DateTimeField
                      label={m.logs_to_label()}
                      value={draft.to}
                      onChange={(to) => setDraft((value) => ({ ...value, to }))}
                    />
                  </Suspense>
                </div>
                <div className="flex justify-end gap-2">
                  <Button
                    type="button"
                    onClick={() => setDraft({ target: '', from: '', to: '' })}
                  >
                    {m.common_reset()}
                  </Button>
                  <ModalClose type="button">{m.common_cancel()}</ModalClose>
                  <Button type="submit" variant="flat">
                    {m.common_apply()}
                  </Button>
                </div>
              </form>
            </ModalContent>
          </Modal>
          <LogClearButton
            disabled={logs.loading || !logs.rows.length}
            onClick={() => logs.clear()}
          />
        </>
      }
    >
      <div
        className="flex min-h-0 min-w-0 flex-1 flex-col"
        data-slot="file-logs"
      >
        <LogSearch
          value={search}
          onChange={(value) => {
            setSearch(value)
            setFollowing(true)
          }}
          placeholder={m.logs_filter_placeholder()}
        />
        <div className={logPanelClass}>
          {(logs.loading ||
            logs.loadingOlder ||
            logs.error ||
            logs.page?.partial ||
            (logs.page &&
              (logs.page.malformed !== '0' ||
                logs.page.truncated !== '0'))) && (
            <div
              className="text-on-surface-variant flex flex-wrap items-center gap-2 px-4 py-2 text-xs"
              role="status"
            >
              {logs.loadingOlder && <span>{m.logs_load_older()}…</span>}
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
                (logs.page.malformed !== '0' ||
                  logs.page.truncated !== '0') && (
                  <span>
                    {m.logs_parse_diagnostics({
                      malformed: logs.page.malformed,
                      truncated: logs.page.truncated,
                    })}
                  </span>
                )}
            </div>
          )}
          <ScrollArea className="min-h-0 flex-1">
            <FileRows
              key={`${file}:${JSON.stringify(filter)}`}
              logs={logs}
              search={debounced}
              following={following}
              setFollowing={setFollowing}
              setUnseen={setUnseen}
            />
          </ScrollArea>
          {!following && (
            <LogFollowButton
              unseen={unseen}
              onClick={() => {
                setFollowing(true)
                logs.latest()
              }}
            />
          )}
        </div>
      </div>
    </LogsLayout>
  )
}

function FileRows({
  logs,
  search,
  following,
  setFollowing,
  setUnseen,
}: {
  logs: ReturnType<typeof useFileLogs>
  search: string
  following: boolean
  setFollowing: (value: boolean) => void
  setUnseen: React.Dispatch<React.SetStateAction<number>>
}) {
  const { viewportRef, isBottom, scrollDirection, isTop } = useScrollArea()
  const lastId = useRef<string | undefined>(undefined)
  const anchor = useRef<{
    id: string
    delta: number
    firstId: string | undefined
  } | null>(null)
  const virtualizer = useVirtualizer({
    count: logs.rows.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 110,
    overscan: 5,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
    getItemKey: (index) => logs.rows[index]?.id ?? index,
  })
  const totalSize = virtualizer.getTotalSize()
  useEffect(() => {
    // Reconnect after ScrollArea has attached its viewport ref.
    virtualizer.measure()
  }, [virtualizer])
  useEffect(() => {
    if (scrollDirection === 'up' && !isBottom) setFollowing(false)
  }, [scrollDirection, isBottom, setFollowing])
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      if (following) anchor.current = null
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
  }, [logs.rows, logs.more, following, virtualizer, totalSize])
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
  }, [logs.rows, following, setUnseen])
  useEffect(() => {
    if (logs.loading || logs.loadingOlder || logs.error || !logs.more) return
    const viewport = viewportRef.current
    const fitsViewport =
      viewport && viewport.scrollHeight <= viewport.clientHeight
    if (
      logs.rows.length &&
      !fitsViewport &&
      (following || !isTop || (viewportRef.current?.scrollTop ?? 0) > 0)
    )
      return
    // Fill short filtered pages so history remains reachable without a scrollbar.
    const timer = setTimeout(() => {
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
    }, 100)
    return () => clearTimeout(timer)
  }, [logs, following, isTop, virtualizer, viewportRef, setFollowing])
  return (
    <div>
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
                transform: `translateY(${item.start}px)`,
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
