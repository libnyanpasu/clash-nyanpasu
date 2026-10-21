import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import { useReducedMotion } from 'motion/react'
import {
  lazy,
  memo,
  Suspense,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { Input } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import {
  ScrollArea,
  useScrollArea,
  useScrollAreaViewport,
  useScrollAreaViewportElement,
} from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { useFileLogs } from '@nyanpasu/query'
import {
  type Filter,
  type Level,
  type LogError,
  type LogRow,
  type LogSource,
} from '@nyanpasu/rpc/types'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useDataPageBottomInset } from '../../_modules/data-page'
import { Route } from '../route'
import { LogFileMenu } from './log-file-menu'
import {
  LogClearButton,
  LogFollowButton,
  logPanelVariants,
  LogRecord,
  LogSearch,
  LogVirtualItem,
  useEnteringRows,
  useSlowLoading,
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
const DateTimeField = lazy(() => import('@nyanpasu/ui/date-time-field'))

const logRowKey = (row: LogRow) => row.id

// A row formats its time once when it mounts, not on every list render.
const FileLogRecord = memo(function FileLogRecord({
  row,
  search,
  onInspect,
}: {
  row: LogRow
  search: string
  onInspect: () => void
}) {
  const time = row.timestamp
    ? new Date(Number(row.timestamp)).toLocaleString()
    : undefined
  return (
    <LogRecord
      time={time ?? '—'}
      timeTitle={time}
      level={row.level}
      target={row.target}
      message={row.message}
      raw={row.raw}
      search={search}
      incomplete={row.unparsed || row.truncated}
      onInspect={onInspect}
    />
  )
})

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
  const slowLoading = useSlowLoading(logs.loading)
  const filterCount = [target, from, to].filter(Boolean).length
  const selectFile = (value: string | null) => {
    setFollowing(true)
    setFile(value)
  }
  return (
    <LogsLayout
      source={source}
      search={
        <LogSearch
          value={search}
          onChange={(value) => {
            setSearch(value)
            setFollowing(true)
          }}
          placeholder={m.logs_filter_placeholder()}
        />
      }
      actions={
        <>
          <LogFileMenu
            file={file}
            files={logs.files}
            onValueChange={selectFile}
          />
          <Modal
            open={filterOpen}
            onOpenChange={(open) => {
              if (open) setDraft({ target, from, to })
              setFilterOpen(open)
            }}
          >
            <ModalTrigger asChild>
              <Button
                variant="glass"
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
                      locale={getLocale()}
                      openCalendarLabel={m.date_time_open_calendar()}
                      clearLabel={m.common_clear()}
                      label={m.logs_from_label()}
                      value={draft.from}
                      onChange={(from) =>
                        setDraft((value) => ({ ...value, from }))
                      }
                    />
                    <DateTimeField
                      locale={getLocale()}
                      openCalendarLabel={m.date_time_open_calendar()}
                      clearLabel={m.common_clear()}
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
      <div className={logPanelVariants()} data-slot="file-logs">
        {(slowLoading ||
          logs.loadingOlder ||
          logs.error ||
          logs.page?.partial ||
          (logs.page &&
            (logs.page.malformed !== '0' || logs.page.truncated !== '0'))) && (
          <div
            className="text-on-surface-variant flex flex-wrap items-center gap-2 px-4 py-2 text-xs"
            role="status"
          >
            {logs.loadingOlder && <span>{m.logs_load_older()}…</span>}
            {slowLoading && (
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
  const { isBottom, scrollDirection, isTop } = useScrollArea()
  const { viewportRef } = useScrollAreaViewport()
  const viewportElement = useScrollAreaViewportElement()
  // A page from IPC, or a tail poll, mounts and measures new rows; deferring
  // the rows keeps that render from blocking scrolling and input. Everything
  // indexing the virtual list reads this one array so the indexes agree.
  const rows = useDeferredValue(logs.rows)
  const bottomInset = useDataPageBottomInset()
  const reducedMotion = useReducedMotion()
  const followScrollBehavior = useRef<ScrollBehavior>('auto')
  // Rows mount in a deferred render, so a remount for another file or
  // filter commits at once instead of rendering the previous rows first.
  const showRows = useDeferredValue(true, false)
  const isEntering = useEnteringRows(rows, logRowKey)
  const stopFollowing = useCallback(() => setFollowing(false), [setFollowing])
  const lastId = useRef<string | undefined>(undefined)
  const anchor = useRef<{
    id: string
    delta: number
    firstId: string | undefined
  } | null>(null)
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewportElement,
    estimateSize: () => 110,
    paddingEnd: bottomInset,
    scrollPaddingEnd: bottomInset,
    overscan: 5,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
    getItemKey: (index) => rows[index]?.id ?? index,
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
      // Start instantly; resuming follow mode animates, including size corrections.
      if (!following) followScrollBehavior.current = 'smooth'
      if (following) anchor.current = null
      if (anchor.current) {
        if (rows !== logs.rows) return
        if (rows[0]?.id === anchor.current.firstId && logs.more) return
        const index = rows.findIndex((row) => row.id === anchor.current?.id)
        if (index >= 0) {
          const offset = virtualizer.getOffsetForIndex(index, 'start')
          if (offset)
            virtualizer.scrollToOffset(offset[0] + anchor.current.delta)
        }
        anchor.current = null
      } else if (following && rows.length) {
        virtualizer.scrollToIndex(rows.length - 1, {
          align: 'end',
          behavior: reducedMotion ? 'auto' : followScrollBehavior.current,
        })
      }
    })
    return () => cancelAnimationFrame(frame)
  }, [
    rows,
    logs.rows,
    logs.more,
    following,
    virtualizer,
    totalSize,
    reducedMotion,
  ])
  useEffect(() => {
    const latest = rows.at(-1)?.id
    if (!following && latest && lastId.current && latest !== lastId.current) {
      const previous = rows.findIndex((row) => row.id === lastId.current)
      setUnseen(
        (count) => count + (previous < 0 ? 1 : rows.length - previous - 1),
      )
    }
    lastId.current = latest
    if (following) setUnseen(0)
  }, [rows, following, setUnseen])
  useEffect(() => {
    if (logs.loading || logs.loadingOlder || logs.error || !logs.more) return
    // Judge the page by rows on screen, not ones still rendering.
    if (rows !== logs.rows) return
    const viewport = viewportRef.current
    const fitsViewport =
      viewport && viewport.scrollHeight <= viewport.clientHeight
    if (
      rows.length &&
      !fitsViewport &&
      (following || !isTop || (viewportRef.current?.scrollTop ?? 0) > 0)
    )
      return
    // Fill short filtered pages so history remains reachable without a scrollbar.
    const timer = setTimeout(() => {
      const first = virtualizer
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
      logs.loadOlder()
    }, 100)
    return () => clearTimeout(timer)
  }, [logs, rows, following, isTop, virtualizer, viewportRef, setFollowing])
  return (
    <div>
      {!logs.rows.length && !logs.loading && !logs.error && (
        <EmptyState
          className="py-16"
          data-slot="logs-no-logs"
          message={
            logs.more ? m.logs_search_incomplete() : m.logs_empty_message()
          }
        />
      )}
      <div className="relative" style={{ height: totalSize }}>
        {showRows &&
          virtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index]
            if (!row) return null
            return (
              <LogVirtualItem
                key={row.id}
                ref={virtualizer.measureElement}
                data-index={item.index}
                entering={isEntering(row)}
                style={{
                  transform: `translateY(${item.start}px)`,
                }}
              >
                <FileLogRecord
                  row={row}
                  search={search}
                  onInspect={stopFollowing}
                />
              </LogVirtualItem>
            )
          })}
      </div>
    </div>
  )
}
