import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import {
  memo,
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
  type Column,
  type ColumnDef,
  type ColumnSizingState,
  type Row,
  type RowData,
  type Updater,
} from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useLocalStorage } from '@uidotdev/usehooks'
import { RowsTickContext } from './cells'
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

export type RowProps = ComponentProps<'tr'>

type RenderRow<TRow> = (row: TRow, props: RowProps) => ReactNode

const renderPlainRow = (_: unknown, props: RowProps) => <tr {...props} />

const COLUMN_SIZING_STORAGE_KEY = 'connections-column-sizing-v2'

// Rows have a fixed height (the cells' h-9), so the virtualizer measures nothing.
const ROW_HEIGHT = 36

// Rows closer than this to the end load the next page.
const END_REACHED_THRESHOLD = 20

const sameItems = <T,>(a: readonly T[], b: readonly T[]) =>
  a.length === b.length && a.every((item, index) => item === b[index])

type BodyRowProps<TRow extends RowData> = {
  row: Row<typeof features, TRow>
  // Compared only: the visible columns, in order, and their widths.
  columns: ReadonlyArray<Column<typeof features, TRow>>
  widths: readonly number[]
  offset: number
  renderRow: RenderRow<TRow>
  isRowEqual: (a: TRow, b: TRow) => boolean
}

// A new sample replaces every row object, but most rows show the same values
// as before, so a row re-renders only when `isRowEqual` tells its data apart
// or the columns change.
const BodyRow = memo(
  function BodyRow<TRow extends RowData>({
    row,
    widths,
    offset,
    renderRow,
  }: BodyRowProps<TRow>) {
    return renderRow(row.original, {
      className: cn(
        'transition-colors',
        'hover:bg-primary/5 active:bg-primary/10',
      ),
      style: {
        height: `${ROW_HEIGHT}px`,
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
          style={{ width: widths[index] }}
        >
          {flexRender(cell.column.columnDef.cell, cell.getContext())}
        </td>
      )),
    })
  },
  (prev, next) =>
    prev.offset === next.offset &&
    prev.renderRow === next.renderRow &&
    prev.isRowEqual === next.isRowEqual &&
    sameItems(prev.columns, next.columns) &&
    sameItems(prev.widths, next.widths) &&
    next.isRowEqual(prev.row.original, next.row.original),
) as <TRow extends RowData>(props: BodyRowProps<TRow>) => ReactNode

function ConnectionsTable<TRow extends RowData>({
  settingsKey,
  columns,
  data,
  getRowId,
  renderRow = renderPlainRow,
  isRowEqual = Object.is,
  emptyMessage,
  onEndReached,
  settingsOpen,
  onSettingsOpenChange,
}: {
  settingsKey: string
  columns: Array<ConnectionColumn<TRow>>
  data: TRow[]
  getRowId: (row: TRow) => string
  renderRow?: RenderRow<TRow>
  // Whether a row from a new sample shows the same as the one it replaces.
  isRowEqual?: (a: TRow, b: TRow) => boolean
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
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
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

  const visibleColumns = table.getVisibleLeafColumns()
  const visibleColumnCount = visibleColumns.length
  const tableBaseWidth = table.getTotalSize()
  const extraWidthPerColumn =
    visibleColumnCount > 0 && viewportWidth > tableBaseWidth
      ? (viewportWidth - tableBaseWidth) / visibleColumnCount
      : 0
  const tableRenderWidth = Math.max(tableBaseWidth, viewportWidth)
  const cellWidths = visibleColumns.map(
    (column) => column.getSize() + extraWidthPerColumn,
  )

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
            <RowsTickContext.Provider value={data}>
              {virtualItems.map((virtualRow, index) => {
                const row = rows[virtualRow.index]

                if (!row) {
                  return null
                }

                return (
                  <BodyRow
                    key={row.id}
                    row={row}
                    columns={visibleColumns}
                    widths={cellWidths}
                    offset={virtualRow.start - index * virtualRow.size}
                    renderRow={renderRow}
                    isRowEqual={isRowEqual}
                  />
                )
              })}
            </RowsTickContext.Provider>
          </tbody>
        </table>
      </div>

      {settingsModal}
    </>
  )
}

// A deferred sample first renders with the previous one; memoized, the table
// skips that render, as its props stay the same.
export default memo(ConnectionsTable) as typeof ConnectionsTable
