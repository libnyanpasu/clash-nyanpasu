import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import dayjs from 'dayjs'
import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { Button } from '@/components/ui/button'
import { ContextMenuItem } from '@/components/ui/context-menu'
import HighlightText from '@/components/ui/highlight-text'
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { containsSearchTerm } from '@/utils'
import parseTraffic from '@/utils/parse-traffic'
import { ClashConnectionItem, useClashConnections } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import {
  ColumnDef,
  ColumnSizingState,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type Updater,
} from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useLocalStorage } from '@uidotdev/usehooks'
import TableRow from './_modules/table-row'
import { Route as IndexRoute } from './route'

export type ConnectionRow = ClashConnectionItem & {
  closed: boolean
  downloadSpeed: number
  uploadSpeed: number
}

const COLUMN_SIZING_STORAGE_KEY = 'connections-column-sizing-v2'

export const Route = createFileRoute('/(main)/main/connections/')({
  component: RouteComponent,
})

const Viewer = ({ search }: { search: string }) => {
  const { proxy } = IndexRoute.useSearch()

  const [columnSizing, setColumnSizing] = useLocalStorage<ColumnSizingState>(
    COLUMN_SIZING_STORAGE_KEY,
    {},
  )

  const {
    query: { data: clashConnections },
  } = useClashConnections()

  const { viewportRef } = useScrollArea()

  const data = useMemo<ConnectionRow[]>(() => {
    const allSnapshots = clashConnections ?? []

    const latestConnections = allSnapshots.at(-1)?.connections ?? []
    const prevConnections = allSnapshots.at(-2)?.connections ?? []

    const prevMap = new Map(prevConnections.map((c) => [c.id, c]))

    const all = latestConnections
      .filter((conn) => (proxy ? conn.chains?.includes(proxy) : true))
      .map((conn) => {
        const prev = prevMap.get(conn.id)
        return {
          ...conn,
          closed: false,
          downloadSpeed: prev ? conn.download - prev.download : 0,
          uploadSpeed: prev ? conn.upload - prev.upload : 0,
        }
      })
      .filter((c) => (search ? containsSearchTerm(c, search) : true))

    return all
  }, [clashConnections, search, proxy])

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
          header: 'Host',
          accessorFn: ({ metadata }) => metadata.host || metadata.destinationIP,
          size: 320,
          cell: (info) => (
            <HighlightText searchText={search}>
              {info.row.original.metadata.host ||
                info.row.original.metadata.destinationIP ||
                ''}
            </HighlightText>
          ),
        },
        {
          header: 'Chains',
          accessorFn: ({ chains }) => [...chains].reverse().join(' / '),
          size: 360,
          cell: (info) => (
            <HighlightText searchText={search}>
              {[...info.row.original.chains].reverse().join(' / ') || ''}
            </HighlightText>
          ),
        },

        {
          header: 'Downloaded',
          accessorFn: ({ download }) => parseTraffic(download).join(' '),
          sortingFn: (rowA, rowB) =>
            rowA.original.download - rowB.original.download,
          size: 120,
          cell: (info) => (
            <HighlightText searchText={search}>
              {parseTraffic(info.row.original.download).join(' ')}
            </HighlightText>
          ),
        },
        {
          header: 'Uploaded',
          accessorFn: ({ upload }) => parseTraffic(upload).join(' '),
          sortingFn: (rowA, rowB) =>
            rowA.original.upload - rowB.original.upload,
          size: 120,
          cell: (info) => (
            <span>{parseTraffic(info.row.original.upload).join(' ')}</span>
          ),
        },
        {
          header: 'DL Speed',
          accessorFn: ({ downloadSpeed }) =>
            parseTraffic(downloadSpeed).join(' ') + '/s',
          sortingFn: (rowA, rowB) =>
            rowA.original.downloadSpeed - rowB.original.downloadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.downloadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          header: 'UL Speed',
          accessorFn: ({ uploadSpeed }) =>
            parseTraffic(uploadSpeed).join(' ') + '/s',
          sortingFn: (rowA, rowB) =>
            rowA.original.uploadSpeed - rowB.original.uploadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.uploadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          header: 'Process',
          accessorFn: ({ metadata }) => metadata.process,
          size: 160,
          cell: (info) => (
            <HighlightText searchText={search}>
              {info.row.original.metadata.process || ''}
            </HighlightText>
          ),
        },
        {
          header: 'Rule',
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
          header: 'Time',
          accessorFn: ({ start }) => dayjs(start).fromNow(),
          sortingFn: (rowA, rowB) =>
            dayjs(rowA.original.start).diff(rowB.original.start),
          size: 120,
          cell: (info) => (
            <span
              title={dayjs(info.row.original.start).format(
                'YYYY-MM-DD HH:mm:ss',
              )}
            >
              {dayjs(info.row.original.start).fromNow()}
            </span>
          ),
        },
        {
          header: 'Source',
          accessorFn: ({ metadata: { sourceIP, sourcePort } }) =>
            `${sourceIP}:${sourcePort}`,
          size: 160,
          cell: (info) => (
            <HighlightText searchText={search}>
              {`${info.row.original.metadata.sourceIP}:${info.row.original.metadata.sourcePort}`}
            </HighlightText>
          ),
        },
        {
          header: 'Destination IP',
          accessorFn: ({ metadata: { destinationIP, destinationPort } }) =>
            `${destinationIP}:${destinationPort}`,
          size: 160,
          cell: (info) => (
            <HighlightText searchText={search}>
              {`${info.row.original.metadata.destinationIP || ''}:${info.row.original.metadata.destinationPort || ''}`}
            </HighlightText>
          ),
        },
        {
          header: 'Type',
          accessorFn: ({ metadata }) =>
            `${metadata.type} (${metadata.network})`,
          size: 120,
          cell: (info) => (
            <HighlightText searchText={search}>
              {`${info.row.original.metadata.type} (${info.row.original.metadata.network})`}
            </HighlightText>
          ),
        },
      ] satisfies Array<ColumnDef<ConnectionRow>>,
    [search],
  )

  const table = useReactTable({
    data,
    columns,
    state: {
      columnSizing,
    },
    onColumnSizingChange: handleColumnSizingChange,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
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

  if (rows.length === 0) {
    return (
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
    )
  }

  return (
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
              >
                {row.getVisibleCells().map(({ column, id, getContext }) => (
                  <td
                    key={id}
                    className="border-outline-variant/30 max-w-0 truncate border-b px-3 text-sm"
                    style={{ width: column.getSize() + extraWidthPerColumn }}
                  >
                    {flexRender(column.columnDef.cell, getContext())}
                  </td>
                ))}
              </TableRow>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

function RouteComponent() {
  const [search, setSearch] = useState('')

  const { deleteConnections } = useClashConnections()

  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(null)
  })

  return (
    <div className="divide-outline-variant flex h-full min-h-0 flex-1 flex-col divide-y overflow-hidden">
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <ScrollArea className="min-h-0 flex-1" scrollbars="both" type="hover">
            <Viewer search={search} />
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
