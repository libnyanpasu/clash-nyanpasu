import { useScrollArea } from '@/components/ui/scroll-area'
import { useClashRules } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import {
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'

export const Route = createFileRoute('/(main)/main/rules/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { data } = useClashRules()

  const { viewportRef } = useScrollArea()

  const rowVirtualizer = useVirtualizer({
    count: data?.rules?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 48,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = rowVirtualizer.getVirtualItems()

  const table = useReactTable({
    data: data?.rules ?? [],
    columns: [
      {
        accessorKey: 'Index',
        header: 'Index',
        cell: (info) => info.row.index + 1,
      },
      {
        accessorKey: 'type',
        header: 'Type',
        cell: (info) => info.row.original.type,
      },
      {
        accessorKey: 'payload',
        header: 'Payload',
        cell: (info) => info.row.original.payload,
      },
      {
        accessorKey: 'proxy',
        header: 'Proxy',
        cell: (info) => info.row.original.proxy,
      },
    ],
    // state: {
    //   sorting,
    // },
    // onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    debugTable: true,
  })

  const { rows } = table.getRowModel()

  return (
    <div
      className="mx-auto max-w-7xl px-4"
      data-slot="rules-virtual-container"
      style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
    >
      <table className="w-full table-fixed" data-slot="rules-virtual-table">
        <colgroup>
          <col className="w-20" />
          <col className="w-40" />
          <col />
          <col className="w-40" />
        </colgroup>

        <thead className="h-10" data-slot="rules-virtual-thead">
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id} data-slot="rules-virtual-tr">
              {headerGroup.headers.map(
                ({ id, colSpan, isPlaceholder, column, getContext }) => (
                  <th key={id} data-slot="rules-virtual-th" colSpan={colSpan}>
                    {isPlaceholder ? null : (
                      <div
                        className={cn(
                          'text-left align-middle font-bold select-none',
                        )}
                      >
                        {flexRender(column.columnDef.header, getContext())}
                      </div>
                    )}
                  </th>
                ),
              )}
            </tr>
          ))}
        </thead>

        <tbody className="select-text" data-slot="rules-virtual-tbody">
          {virtualItems.map((virtualRow, index) => {
            const row = rows[virtualRow.index]

            const offset = virtualRow.start - index * virtualRow.size

            return (
              <tr
                key={row.id}
                data-index={virtualRow.index}
                data-slot="rules-virtual-tr"
                style={{
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${offset}px)`,
                }}
              >
                {row.getVisibleCells().map(({ column, id, getContext }) => (
                  <td key={id} data-slot="rules-virtual-td">
                    {flexRender(column.columnDef.cell, getContext())}
                  </td>
                ))}
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
