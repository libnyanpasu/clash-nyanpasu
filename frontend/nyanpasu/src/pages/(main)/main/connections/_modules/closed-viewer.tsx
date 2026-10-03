import { memo, useCallback, useMemo, useState } from 'react'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatRelativeTime } from '@/utils/date'
import parseTraffic from '@/utils/parse-traffic'
import { type ClosedConnection } from '@nyanpasu/rpc/types'
import {
  ChainCell,
  RelativeTimeCell,
  RowsTickContext,
  RuleCell,
  TextCell,
  TrafficCell,
} from './cells'
import ConnectionsTable, {
  type ConnectionColumn,
  type RowProps,
} from './connections-table'
import TableRow, {
  closedConnectionDetail,
  ConnectionDetailModal,
} from './table-row'
import {
  closedConnectionId,
  useClosedConnectionRows,
  type ConnectionsSelection,
} from './use-connection-rows'

// A closed record never changes, so a row keyed by it always shows the same.
const sameRecord = () => true

// The traffic history keeps the process path; the table shows its name.
const processName = (process: string) => process.split('/').pop() || process

// Closed connections of the selection. Memoized like the active view, so a
// keystroke's urgent render skips the table.
const ClosedViewer = memo(function ClosedViewer({
  search,
  proxy,
  selection,
  settingsOpen,
  onSettingsOpenChange,
}: {
  search: string
  proxy?: string | null
  selection: ConnectionsSelection
  settingsOpen: boolean
  onSettingsOpenChange: (open: boolean) => void
}) {
  const { rows, error, hasNextPage, onEndReached } = useClosedConnectionRows({
    search,
    proxy,
    selection,
  })

  // A closed record never changes, so the dialog keeps the row itself.
  const [detailRow, setDetailRow] = useState<ClosedConnection | null>(null)

  const detail = useMemo(
    () => (detailRow === null ? undefined : closedConnectionDetail(detailRow)),
    [detailRow],
  )

  const closeDetail = useCallback(() => setDetailRow(null), [])

  const renderRow = useCallback(
    (row: ClosedConnection, props: RowProps) => (
      <TableRow {...props} onViewDetails={() => setDetailRow(row)} />
    ),
    [],
  )

  const columns = useMemo(
    () =>
      [
        {
          // ids match the active table's columns so both share column sizing
          id: 'Host',
          header: () => m.connections_column_host(),
          accessorFn: ({ dimensions }) => dimensions.target,
          size: 280,
          cell: (info) => (
            <TextCell
              value={info.row.original.dimensions.target}
              search={search}
              primary
            />
          ),
        },
        {
          id: 'Chains',
          header: () => m.connections_column_chains(),
          accessorFn: ({ dimensions }) =>
            [...dimensions.chains].reverse().join(' / '),
          size: 320,
          cell: (info) => (
            <ChainCell
              chains={info.row.original.dimensions.chains}
              search={search}
            />
          ),
        },
        {
          id: 'Downloaded',
          header: () => m.connections_column_downloaded(),
          accessorFn: ({ bytes }) => parseTraffic(bytes.download).join(' '),
          sortFn: (rowA, rowB) =>
            rowA.original.bytes.download - rowB.original.bytes.download,
          size: 110,
          meta: { align: 'end' },
          cell: (info) => (
            <TrafficCell value={info.row.original.bytes.download} />
          ),
        },
        {
          id: 'Uploaded',
          header: () => m.connections_column_uploaded(),
          accessorFn: ({ bytes }) => parseTraffic(bytes.upload).join(' '),
          sortFn: (rowA, rowB) =>
            rowA.original.bytes.upload - rowB.original.bytes.upload,
          size: 110,
          meta: { align: 'end' },
          cell: (info) => (
            <TrafficCell value={info.row.original.bytes.upload} />
          ),
        },
        {
          id: 'Process',
          header: () => m.connections_column_process(),
          accessorFn: ({ dimensions }) => processName(dimensions.process),
          size: 160,
          cell: (info) => (
            <span title={info.row.original.dimensions.process}>
              <TextCell
                value={processName(info.row.original.dimensions.process)}
                search={search}
              />
            </span>
          ),
        },
        {
          id: 'Rule',
          header: () => m.connections_column_rule(),
          accessorFn: ({ dimensions: { rule } }) =>
            rule.payload ? `${rule.kind} (${rule.payload})` : rule.kind,
          size: 200,
          cell: (info) => (
            <RuleCell
              kind={info.row.original.dimensions.rule.kind}
              payload={info.row.original.dimensions.rule.payload}
              search={search}
            />
          ),
        },
        {
          id: 'Time',
          header: () => m.connections_field_start(),
          accessorFn: ({ started_at }) =>
            formatRelativeTime(started_at, Date.now(), getLocale()),
          sortFn: (rowA, rowB) =>
            rowA.original.started_at - rowB.original.started_at,
          size: 110,
          cell: (info) => (
            <RelativeTimeCell ms={info.row.original.started_at} />
          ),
        },
        {
          id: 'Closed',
          header: () => m.connections_column_closed_time(),
          accessorFn: ({ closed_at }) =>
            formatRelativeTime(closed_at, Date.now(), getLocale()),
          sortFn: (rowA, rowB) =>
            rowA.original.closed_at - rowB.original.closed_at,
          size: 110,
          cell: (info) => <RelativeTimeCell ms={info.row.original.closed_at} />,
        },
        {
          id: 'Source',
          header: () => m.connections_column_source(),
          accessorFn: ({ dimensions }) => dimensions.source,
          size: 160,
          cell: (info) => (
            <TextCell
              value={info.row.original.dimensions.source}
              search={search}
            />
          ),
        },
        {
          id: 'Type',
          header: () => m.connections_column_type(),
          accessorFn: ({ dimensions }) => dimensions.protocol,
          size: 120,
          cell: (info) => (
            <TextCell
              value={info.row.original.dimensions.protocol}
              search={search}
            />
          ),
        },
      ] satisfies Array<ConnectionColumn<ClosedConnection>>,
    [search],
  )

  return (
    <>
      <ConnectionsTable
        settingsKey="connections-columns-closed"
        columns={columns}
        data={rows}
        getRowId={closedConnectionId}
        renderRow={renderRow}
        isRowEqual={sameRecord}
        emptyMessage={
          error
            ? m.connections_closed_unavailable()
            : hasNextPage
              ? m.connections_closed_loading()
              : m.connections_empty_message()
        }
        onEndReached={onEndReached}
        settingsOpen={settingsOpen}
        onSettingsOpenChange={onSettingsOpenChange}
      />

      <RowsTickContext.Provider value={rows}>
        <ConnectionDetailModal detail={detail} onClose={closeDetail} />
      </RowsTickContext.Provider>
    </>
  )
})

export default ClosedViewer
