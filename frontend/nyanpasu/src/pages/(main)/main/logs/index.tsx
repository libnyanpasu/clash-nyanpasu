import AppsRounded from '~icons/material-symbols/apps-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import MemoryRounded from '~icons/material-symbols/memory-rounded'
import { useEffect, useMemo, useState } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import AnimatedTabs, { AnimatedTabsItem } from '@/components/ui/animated-tabs'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { useClashLogs } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import FileLogs from './_modules/file-logs'
import {
  LogEmptyState,
  logPanelClass,
  LogRecord,
  LogSearch,
  LogViewerActions,
  useLogToolbar,
} from './_modules/log-viewer'
import { Route as IndexRoute } from './route'

export const Route = createFileRoute('/(main)/main/logs/')({
  component: RouteComponent,
})

const Viewer = ({
  search,
  onClear,
}: {
  search: string
  onClear: () => void
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
  const [following, setFollowing] = useState(true)
  const toolbar = useLogToolbar()
  const rowVirtualizer = useVirtualizer({
    count: filteredLogs.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 110,
    overscan: 5,
    scrollMargin: toolbar.height,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
  })
  const totalSize = rowVirtualizer.getTotalSize()
  useEffect(() => {
    if (scrollDirection === 'up' && !isBottom) setFollowing(false)
  }, [scrollDirection, isBottom])
  useEffect(() => {
    if (!following || !filteredLogs.length) return
    const frame = requestAnimationFrame(() =>
      rowVirtualizer.scrollToIndex(filteredLogs.length - 1, { align: 'end' }),
    )
    return () => cancelAnimationFrame(frame)
  }, [filteredLogs, following, rowVirtualizer, totalSize, toolbar.height])
  return (
    <div>
      <LogViewerActions
        ref={toolbar.ref}
        following={following}
        clearDisabled={!logs?.length}
        onClear={onClear}
        onFollow={() => {
          setFollowing(true)
          if (filteredLogs.length)
            rowVirtualizer.scrollToIndex(filteredLogs.length - 1, {
              align: 'end',
            })
        }}
      />
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
                transform: `translateY(${item.start - toolbar.height}px)`,
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
  const [search, setSearch] = useState('')
  const { data: logs, clean } = useClashLogs()
  const handleClearLogs = useLockFn(async () => {
    await clean.mutateAsync()
  })
  return (
    <>
      <LogSearch
        value={search}
        onChange={setSearch}
        placeholder={m.logs_search_placeholder()}
      />
      <div className={logPanelClass}>
        <RegisterContextMenu>
          <RegisterContextMenuTrigger asChild>
            <ScrollArea className="min-h-0 flex-1">
              <Viewer search={search} onClear={handleClearLogs} />
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
      </div>
    </>
  )
}

function RouteComponent() {
  const { source = 'core' } = IndexRoute.useSearch()
  const navigate = IndexRoute.useNavigate()
  const icons = { core: MemoryRounded, app: AppsRounded, service: DnsRounded }
  const labels = {
    core: m.logs_source_core(),
    app: m.logs_source_app(),
    service: m.logs_source_service(),
  }
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div
        className="shrink-0 p-3"
        role="group"
        aria-label={m.logs_source_label()}
      >
        <AnimatedTabs
          variant="segment"
          activeTab={source}
          className="w-full sm:max-w-md"
          onChange={(item) => {
            if (item === 'core' || item === 'app' || item === 'service')
              navigate({
                search: (previous) => ({ ...previous, source: item }),
              })
          }}
        >
          {(['core', 'app', 'service'] as const).map((item) => {
            const Icon = source === item ? CheckRounded : icons[item]
            return (
              <AnimatedTabsItem
                key={item}
                value={item}
                id={`logs-source-${item}`}
                aria-controls="logs-source-panel"
              >
                <Icon aria-hidden className="hidden size-5 shrink-0 sm:block" />
                <span>{labels[item]}</span>
              </AnimatedTabsItem>
            )
          })}
        </AnimatedTabs>
      </div>
      <div
        id="logs-source-panel"
        role="tabpanel"
        aria-labelledby={`logs-source-${source}`}
        className="flex min-h-0 min-w-0 flex-1 flex-col"
      >
        {source === 'core' ? (
          <KernelLogs />
        ) : (
          <FileLogs key={source} source={source} />
        )}
      </div>
    </div>
  )
}
