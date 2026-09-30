import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import dayjs from 'dayjs'
import {
  memo,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
} from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { Button } from '@/components/ui/button'
import { ContextMenuItem } from '@/components/ui/context-menu'
import HighlightText from '@/components/ui/highlight-text'
import { ScrollArea, useScrollAreaViewport } from '@/components/ui/scroll-area'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { containsSearchTerm } from '@/utils'
import parseTraffic from '@/utils/parse-traffic'
import {
  ClashConnection_Serialize,
  useClashConnectionDetails,
  useDeleteClashConnections,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import {
  columnResizingFeature,
  columnSizingFeature,
  columnVisibilityFeature,
  createSortedRowModel,
  flexRender,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type ColumnDef,
  type ColumnSizingState,
  type Updater,
} from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useLocalStorage } from '@uidotdev/usehooks'
import TableRow, { ConnectionDetailModal } from './_modules/table-row'
import { Route as IndexRoute } from './route'

export type ConnectionRow = ClashConnection_Serialize & {
  closed: boolean
  // Parsed once per sample: sorting by time compares numbers instead of
  // parsing both dates in every comparison.
  startMs: number
}

const features = tableFeatures({
  rowSortingFeature,
  columnSizingFeature,
  columnResizingFeature,
  columnVisibilityFeature,
  sortedRowModel: createSortedRowModel(),
})

const COLUMN_SIZING_STORAGE_KEY = 'connections-column-sizing-v2'

export const Route = createFileRoute('/(main)/main/connections/')({
  component: RouteComponent,
})

// Memoized so a keystroke's urgent render skips the table; it re-renders
// with the deferred search term, or on its own stream and route updates.
const Viewer = memo(function Viewer({ search }: { search: string }) {
  const { proxy } = IndexRoute.useSearch()

  const [columnSizing, setColumnSizing] = useLocalStorage<ColumnSizingState>(
    COLUMN_SIZING_STORAGE_KEY,
    {},
  )

  const { data: details } = useClashConnectionDetails()

  const { viewportRef } = useScrollAreaViewport()

  const data = useMemo<ConnectionRow[]>(() => {
    const connections = details?.connections ?? []

    return connections
      .filter((conn) => (proxy ? conn.chains?.includes(proxy) : true))
      .map((conn) => ({
        ...conn,
        closed: false,
        startMs: conn.start ? Date.parse(conn.start) : Number.NaN,
      }))
      .filter((c) => (search ? containsSearchTerm(c, search) : true))
  }, [details, search, proxy])

  const [detailId, setDetailId] = useState<string | null>(null)

  const detailRow = useMemo(
    () =>
      detailId === null ? undefined : data.find((row) => row.id === detailId),
    [data, detailId],
  )

  const handleColumnSizingChange = useCallback(
    (updater: Updater<ColumnSizingState>) => {
      setColumnSizing((prev) => {
        return typeof updater === 'function' ? updater(prev) : updater
      })
    },
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
    [],
  )

  const columns = useMemo(
    () =>
      [
        {
          // ids keep the former English headers so persisted column sizing still applies
          id: 'Host',
          header: () => m.connections_column_host(),
          accessorFn: ({ metadata }) =>
            metadata?.host || metadata?.destinationIP,
          size: 320,
          cell: (info) => (
            <HighlightText searchText={search}>
              {info.row.original.metadata?.host ||
                info.row.original.metadata?.destinationIP ||
                ''}
            </HighlightText>
          ),
        },
        {
          id: 'Chains',
          header: () => m.connections_column_chains(),
          accessorFn: ({ chains }) => [...chains].reverse().join(' / '),
          size: 360,
          cell: (info) => (
            <HighlightText searchText={search}>
              {[...info.row.original.chains].reverse().join(' / ') || ''}
            </HighlightText>
          ),
        },

        {
          id: 'Downloaded',
          header: () => m.connections_column_downloaded(),
          accessorFn: ({ download }) => parseTraffic(download).join(' '),
          sortFn: (rowA, rowB) =>
            rowA.original.download - rowB.original.download,
          size: 120,
          cell: (info) => (
            <HighlightText searchText={search}>
              {parseTraffic(info.row.original.download).join(' ')}
            </HighlightText>
          ),
        },
        {
          id: 'Uploaded',
          header: () => m.connections_column_uploaded(),
          accessorFn: ({ upload }) => parseTraffic(upload).join(' '),
          sortFn: (rowA, rowB) => rowA.original.upload - rowB.original.upload,
          size: 120,
          cell: (info) => (
            <span>{parseTraffic(info.row.original.upload).join(' ')}</span>
          ),
        },
        {
          id: 'DL Speed',
          header: () => m.connections_column_download_speed(),
          accessorFn: ({ downloadSpeed }) =>
            parseTraffic(downloadSpeed).join(' ') + '/s',
          sortFn: (rowA, rowB) =>
            rowA.original.downloadSpeed - rowB.original.downloadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.downloadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          id: 'UL Speed',
          header: () => m.connections_column_upload_speed(),
          accessorFn: ({ uploadSpeed }) =>
            parseTraffic(uploadSpeed).join(' ') + '/s',
          sortFn: (rowA, rowB) =>
            rowA.original.uploadSpeed - rowB.original.uploadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.uploadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          id: 'Process',
          header: () => m.connections_column_process(),
          accessorFn: ({ metadata }) => metadata?.process,
          size: 160,
          cell: (info) => (
            <HighlightText searchText={search}>
              {info.row.original.metadata?.process || ''}
            </HighlightText>
          ),
        },
        {
          id: 'Rule',
          header: () => m.connections_column_rule(),
          accessorFn: ({ rule, rulePayload }) =>
            rulePayload ? `${rule} (${rulePayload})` : rule,
          size: 200,
          cell: (info) => (
            <HighlightText searchText={search}>
              {info.row.original.rulePayload
                ? `${info.row.original.rule} (${info.row.original.rulePayload})`
                : info.row.original.rule || ''}
            </HighlightText>
          ),
        },
        {
          id: 'Time',
          header: () => m.connections_column_time(),
          accessorFn: ({ start }) => dayjs(start).fromNow(),
          sortFn: (rowA, rowB) => rowA.original.startMs - rowB.original.startMs,
          size: 120,
          cell: (info) => (
            <span
              title={dayjs(info.row.original.start ?? '').format(
                'YYYY-MM-DD HH:mm:ss',
              )}
            >
              {info.row.original.start
                ? dayjs(info.row.original.start).fromNow()
                : '-'}
            </span>
          ),
        },
        {
          id: 'Source',
          header: () => m.connections_column_source(),
          accessorFn: ({ metadata }) =>
            `${metadata?.sourceIP ?? ''}:${metadata?.sourcePort ?? ''}`,
          size: 160,
          cell: (info) => (
            <HighlightText searchText={search}>
              {`${info.row.original.metadata?.sourceIP ?? ''}:${info.row.original.metadata?.sourcePort ?? ''}`}
            </HighlightText>
          ),
        },
        {
          id: 'Destination IP',
          header: () => m.connections_column_destination(),
          accessorFn: ({ metadata }) =>
            `${metadata?.destinationIP ?? ''}:${metadata?.destinationPort ?? ''}`,
          size: 160,
          cell: (info) => (
            <HighlightText searchText={search}>
              {`${info.row.original.metadata?.destinationIP || ''}:${info.row.original.metadata?.destinationPort || ''}`}
            </HighlightText>
          ),
        },
        {
          id: 'Type',
          header: () => m.connections_column_type(),
          accessorFn: ({ metadata }) =>
            `${metadata?.type ?? ''} (${metadata?.network ?? ''})`,
          size: 120,
          cell: (info) => (
            <HighlightText searchText={search}>
              {`${info.row.original.metadata?.type ?? ''} (${info.row.original.metadata?.network ?? ''})`}
            </HighlightText>
          ),
        },
      ] satisfies Array<ColumnDef<typeof features, ConnectionRow>>,
    [search],
  )

  const table = useTable({
    features,
    data,
    columns,
    // Row identity must follow the connection, not its position in the
    // current sample, or rows and their menus are reused for other connections.
    getRowId: (row) => row.id,
    state: {
      columnSizing,
    },
    onColumnSizingChange: handleColumnSizingChange,
    enableColumnResizing: true,
    columnResizeMode: 'onChange',
  })

  const { rows } = table.getRowModel()

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 40,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = rowVirtualizer.getVirtualItems()

  const [viewportWidth, setViewportWidth] = useState(0)

  useEffect(() => {
    const viewport = viewportRef.current

    if (!viewport) {
      return
    }

    const updateWidth = () => {
      setViewportWidth(viewport.clientWidth)
    }

    updateWidth()

    const observer = new ResizeObserver(updateWidth)
    observer.observe(viewport)

    return () => {
      observer.disconnect()
    }
  }, [viewportRef])

  const visibleColumnCount = table.getVisibleLeafColumns().length
  const tableBaseWidth = table.getTotalSize()
  const extraWidthPerColumn =
    visibleColumnCount > 0 && viewportWidth > tableBaseWidth
      ? (viewportWidth - tableBaseWidth) / visibleColumnCount
      : 0
  const tableRenderWidth = Math.max(tableBaseWidth, viewportWidth)

  const detailModal = (
    <ConnectionDetailModal data={detailRow} onClose={() => setDetailId(null)} />
  )

  if (rows.length === 0) {
    return (
      <>
        <div
          className="absolute inset-0 flex flex-col items-center justify-center gap-4"
          data-slot="connections-no-connections"
        >
          <BoxOutlineRounded className="text-surface-variant size-16" />

          <p
            className="text-surface-variant text-sm"
            data-slot="connections-no-connections-message"
          >
            {m.connections_empty_message()}
          </p>
        </div>

        {detailModal}
      </>
    )
  }

  return (
    <>
      <div
        className="mx-auto min-h-full"
        data-slot="connections-virtual-container"
        style={{
          height: `${rowVirtualizer.getTotalSize()}px`,
        }}
      >
        <table
          className="divide-outline-variant w-full table-fixed border-separate border-spacing-0"
          data-slot="connections-virtual-table"
          style={{ width: tableRenderWidth }}
        >
          <thead className="bg-mixed-background sticky top-0 z-20 h-10">
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => (
                  <th
                    key={header.id}
                    colSpan={header.colSpan}
                    className="border-outline-variant relative border-b whitespace-nowrap"
                    style={{ width: header.getSize() + extraWidthPerColumn }}
                  >
                    {header.isPlaceholder ? null : (
                      <div
                        className={cn(
                          'truncate px-3 text-left align-middle text-sm font-bold select-none',
                          header.column.getCanSort() &&
                            'hover:text-primary cursor-pointer',
                        )}
                        onClick={header.column.getToggleSortingHandler()}
                      >
                        {flexRender(
                          header.column.columnDef.header,
                          header.getContext(),
                        )}
                        {header.column.getIsSorted() === 'asc' && ' ↑'}
                        {header.column.getIsSorted() === 'desc' && ' ↓'}
                      </div>
                    )}
                    {header.column.getCanResize() && (
                      <div
                        onMouseDown={header.getResizeHandler()}
                        onTouchStart={header.getResizeHandler()}
                        className={cn(
                          'absolute top-0 right-0 h-full w-1 cursor-col-resize touch-none select-none',
                          'hover:bg-primary/40 bg-transparent',
                          header.column.getIsResizing() && 'bg-primary/60',
                        )}
                      />
                    )}
                  </th>
                ))}
              </tr>
            ))}
          </thead>

          <tbody className="select-text" data-slot="connections-virtual-tbody">
            {virtualItems.map((virtualRow, index) => {
              const row = rows[virtualRow.index]

              if (!row) {
                return null
              }

              const offset = virtualRow.start - index * virtualRow.size

              return (
                <TableRow
                  key={row.id}
                  data-index={virtualRow.index}
                  ref={(node) => rowVirtualizer.measureElement(node)}
                  className={cn(
                    'transition-colors',
                    'hover:bg-primary/5 active:bg-primary/10',
                    row.original.closed && 'opacity-40',
                  )}
                  style={{
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${offset}px)`,
                  }}
                  data={row.original}
                  onViewDetails={setDetailId}
                >
                  {row.getVisibleCells().map((cell) => (
                    <td
                      key={cell.id}
                      className="border-outline-variant/30 max-w-0 truncate border-b px-3 text-sm"
                      style={{
                        width: cell.column.getSize() + extraWidthPerColumn,
                      }}
                    >
                      {flexRender(
                        cell.column.columnDef.cell,
                        cell.getContext(),
                      )}
                    </td>
                  ))}
                </TableRow>
              )
            })}
          </tbody>
        </table>
      </div>

      {detailModal}
    </>
  )
})

function RouteComponent() {
  const [search, setSearch] = useState('')

  // Filtering, highlighting and re-sorting every connection is heavy; typing
  // stays responsive while the table catches up with the latest term.
  const deferredSearch = useDeferredValue(search)

  const deleteConnections = useDeleteClashConnections()

  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(null)
  })

  return (
    <div className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden">
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <ScrollArea className="min-h-0 flex-1" scrollbars="both" type="hover">
            <Viewer search={deferredSearch} />
          </ScrollArea>
        </RegisterContextMenuTrigger>

        <RegisterContextMenuContent>
          <ContextMenuItem onSelect={() => handleCloseAllConnections()}>
            <CloseRounded className="size-4" />
            <span>{m.connections_close_all_connections()}</span>
          </ContextMenuItem>
        </RegisterContextMenuContent>
      </RegisterContextMenu>

      <div
        className="bg-mixed-background flex h-16 shrink-0 items-center gap-3 px-4"
        data-slot="connections-toolbar"
      >
        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 min-w-0 flex-1 rounded-full px-4 text-sm outline-none',
          )}
          placeholder={m.connections_search_placeholder()}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />

        <Tooltip>
          <TooltipTrigger asChild>
            <Button onClick={handleCloseAllConnections} icon>
              <CloseRounded />
            </Button>
          </TooltipTrigger>

          <TooltipContent>
            {m.connections_close_all_connections()}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}
