import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import {
  Fragment,
  useEffect,
  useMemo,
  useState,
  type ComponentProps,
  type ReactNode,
} from 'react'
import { useScrollAreaViewport } from '@/components/ui/scroll-area'
import { cn } from '@nyanpasu/utils'
import {
  columnOrderingFeature,
  columnResizingFeature,
  columnSizingFeature,
  columnVisibilityFeature,
  createSortedRowModel,
  flexRender,
  functionalUpdate,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type ColumnDef,
  type ColumnSizingState,
  type RowData,
  type Updater,
} from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useLocalStorage } from '@uidotdev/usehooks'
import { useColumnSettings } from './column-settings'
import ColumnSettingsModal from './column-settings-modal'

type ConnectionColumnMeta = {
  // Numbers line up at the end so their magnitudes compare at a glance.
  align?: 'end'
}

const features = tableFeatures({
  rowSortingFeature,
  columnSizingFeature,
  columnResizingFeature,
  columnVisibilityFeature,
  columnOrderingFeature,
  sortedRowModel: createSortedRowModel(),
  columnMeta: {} as ConnectionColumnMeta,
})

// The header is a plain label so the column settings can name hidden columns.
export type ConnectionColumn<TRow extends RowData> = ColumnDef<
  typeof features,
  TRow
> & {
  id: string
  header: () => string
}

type RowProps = ComponentProps<'tr'> & { 'data-index': number }

const COLUMN_SIZING_STORAGE_KEY = 'connections-column-sizing-v2'

// Rows closer than this to the end load the next page.
const END_REACHED_THRESHOLD = 20

export default function ConnectionsTable<TRow extends RowData>({
  settingsKey,
  columns,
  data,
  getRowId,
  renderRow = (_, props) => <tr {...props} />,
  emptyMessage,
  onEndReached,
  settingsOpen,
  onSettingsOpenChange,
}: {
  settingsKey: string
  columns: Array<ConnectionColumn<TRow>>
  data: TRow[]
  getRowId: (row: TRow) => string
  renderRow?: (row: TRow, props: RowProps) => ReactNode
  emptyMessage: string
  onEndReached?: () => void
  settingsOpen: boolean
  onSettingsOpenChange: (open: boolean) => void
}) {
  const [columnSizing, setColumnSizing] = useLocalStorage<ColumnSizingState>(
    COLUMN_SIZING_STORAGE_KEY,
    {},
  )

  const columnIds = useMemo(() => columns.map((column) => column.id), [columns])

  const labels = useMemo(
    () => new Map(columns.map((column) => [column.id, column.header])),
    [columns],
  )

  const { order, visibility, setOrder, setVisibility } = useColumnSettings(
    settingsKey,
    columnIds,
  )

  const { viewportRef } = useScrollAreaViewport()

  const table = useTable({
    features,
    data,
    columns,
    // Row identity must follow the connection, not its position in the
    // current sample, or rows and their menus are reused for other connections.
    getRowId,
    state: {
      columnSizing,
      columnOrder: order,
      columnVisibility: visibility,
    },
    onColumnSizingChange: (updater: Updater<ColumnSizingState>) =>
      setColumnSizing(functionalUpdate(updater, columnSizing)),
    onColumnOrderChange: setOrder,
    onColumnVisibilityChange: setVisibility,
    enableColumnResizing: true,
    columnResizeMode: 'onChange',
  })

  const { rows } = table.getRowModel()

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 36,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = rowVirtualizer.getVirtualItems()

  const lastVirtualIndex = virtualItems.at(-1)?.index

  // An empty table is at its end too: a filter that matches nothing loaded so
  // far keeps loading until it finds a match or runs out of pages.
  useEffect(() => {
    if (
      rows.length === 0 ||
      (lastVirtualIndex !== undefined &&
        lastVirtualIndex >= rows.length - END_REACHED_THRESHOLD)
    ) {
      onEndReached?.()
    }
  }, [lastVirtualIndex, rows.length, onEndReached])

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

  const settingsModal = (
    <ColumnSettingsModal
      open={settingsOpen}
      onOpenChange={onSettingsOpenChange}
      labels={labels}
      order={order}
      visibility={visibility}
      onOrderChange={setOrder}
      onVisibilityChange={setVisibility}
    />
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
            {emptyMessage}
          </p>
        </div>

        {settingsModal}
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
          <thead>
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header, index) => {
                  const sorted = header.column.getIsSorted()

                  return (
                    // Sticky and sized on the cell: WebKit neither sticks nor
                    // sizes a table section as a whole.
                    <th
                      key={header.id}
                      colSpan={header.colSpan}
                      className={cn(
                        'group/th bg-mixed-background sticky top-0 z-20 h-9 p-0',
                        'border-outline-variant border-b',
                      )}
                      style={{ width: header.getSize() + extraWidthPerColumn }}
                    >
                      {header.isPlaceholder ? null : (
                        <div
                          className={cn(
                            'flex h-full items-center gap-1 px-3 select-none',
                            'text-on-surface-variant text-xs font-medium whitespace-nowrap',
                            index === 0 && 'pl-4',
                            header.column.columnDef.meta?.align === 'end' &&
                              'flex-row-reverse',
                            header.column.getCanSort() &&
                              'hover:text-on-surface cursor-pointer',
                            sorted && 'text-primary hover:text-primary',
                          )}
                          onClick={header.column.getToggleSortingHandler()}
                        >
                          <span className="truncate">
                            {flexRender(
                              header.column.columnDef.header,
                              header.getContext(),
                            )}
                          </span>

                          {sorted && (
                            <ArrowUpwardRounded
                              className={cn(
                                'size-3.5 shrink-0',
                                sorted === 'desc' && 'rotate-180',
                              )}
                            />
                          )}
                        </div>
                      )}

                      {header.column.getCanResize() && (
                        <div
                          onMouseDown={header.getResizeHandler()}
                          onTouchStart={header.getResizeHandler()}
                          className="absolute inset-y-0 right-0 flex w-2 cursor-col-resize touch-none items-center justify-end select-none"
                        >
                          <div
                            className={cn(
                              'bg-outline-variant h-4 w-px',
                              'group-hover/th:bg-outline',
                              header.column.getIsResizing() &&
                                'bg-primary group-hover/th:bg-primary w-0.5',
                            )}
                          />
                        </div>
                      )}
                    </th>
                  )
                })}
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
                <Fragment key={row.id}>
                  {renderRow(row.original, {
                    'data-index': virtualRow.index,
                    ref: (node) => rowVirtualizer.measureElement(node),
                    className: cn(
                      'transition-colors',
                      'hover:bg-primary/5 active:bg-primary/10',
                    ),
                    style: {
                      height: `${virtualRow.size}px`,
                      transform: `translateY(${offset}px)`,
                    },
                    children: row.getVisibleCells().map((cell, index) => (
                      <td
                        key={cell.id}
                        className={cn(
                          'border-outline-variant/25 h-9 max-w-0 truncate border-b px-3',
                          'text-on-surface-variant text-[13px]',
                          index === 0 && 'pl-4',
                          cell.column.columnDef.meta?.align === 'end' &&
                            'text-right tabular-nums',
                        )}
                        style={{
                          width: cell.column.getSize() + extraWidthPerColumn,
                        }}
                      >
                        {flexRender(
                          cell.column.columnDef.cell,
                          cell.getContext(),
                        )}
                      </td>
                    )),
                  })}
                </Fragment>
              )
            })}
          </tbody>
        </table>
      </div>

      {settingsModal}
    </>
  )
}
