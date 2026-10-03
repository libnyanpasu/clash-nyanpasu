import { memo, useCallback, useMemo, useRef, useState } from 'react'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatRelativeTime } from '@/utils/date'
import parseTraffic from '@/utils/parse-traffic'
import { useDeleteClashConnections } from '@nyanpasu/query'
import type { SearchFilter } from '../../_modules/traffic-filters'
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
  activeConnectionDetail,
  ConnectionDetailModal,
} from './table-row'
import {
  useActiveConnectionRows,
  type ConnectionRow,
} from './use-connection-rows'

// A connection's other fields are fixed for its life in the core, and its
// relative time follows the table's tick, so only its traffic changes a row.
const sameTraffic = (a: ConnectionRow, b: ConnectionRow) =>
  a.download === b.download &&
  a.upload === b.upload &&
  a.downloadSpeed === b.downloadSpeed &&
  a.uploadSpeed === b.uploadSpeed

const connectionId = (row: ConnectionRow) => row.id

// Memoized so a keystroke's urgent render skips the table; it re-renders
// with the deferred search term, or on its own stream and prop updates.
const ActiveViewer = memo(function ActiveViewer({
  search,
  proxy,
  filters,
  settingsOpen,
  onSettingsOpenChange,
}: {
  search: string
  proxy?: string | null
  filters: SearchFilter[]
  settingsOpen: boolean
  onSettingsOpenChange: (open: boolean) => void
}) {
  const { connections, rows, loading, unavailable } = useActiveConnectionRows({
    search,
    proxy,
    filters,
  })

  const deleteConnections = useDeleteClashConnections()

  // `mutateAsync` is a new function on every render; the row renderer stays
  // the same and calls the latest one, so rows can skip re-rendering.
  const deleteConnectionsRef = useRef(deleteConnections.mutateAsync)
  deleteConnectionsRef.current = deleteConnections.mutateAsync

  const [detailId, setDetailId] = useState<string | null>(null)

  const renderRow = useCallback(
    (row: ConnectionRow, props: RowProps) => (
      <TableRow
        {...props}
        onViewDetails={() => setDetailId(row.id)}
        onCloseConnection={() => deleteConnectionsRef.current(row.id)}
      />
    ),
    [],
  )

  // Looked up unfiltered: a search or proxy filter hiding the row does not
  // close the connection.
  const liveRow = useMemo(
    () =>
      detailId === null
        ? undefined
        : connections.find((row) => row.id === detailId),
    [connections, detailId],
  )

  // The last sample of the open connection, kept so the dialog stays on it
  // after the connection closes and leaves the stream.
  const [lastRow, setLastRow] = useState<ConnectionRow>()

  if (liveRow && liveRow !== lastRow) {
    setLastRow(liveRow)
  }

  const detailRow = liveRow ?? (lastRow?.id === detailId ? lastRow : undefined)

  const detail = useMemo(
    () => detailRow && activeConnectionDetail(detailRow, detailRow !== liveRow),
    [detailRow, liveRow],
  )

  const closeDetail = useCallback(() => setDetailId(null), [])

  const closeDetailConnection = useMemo(
    () =>
      detailId === null
        ? undefined
        : () => deleteConnectionsRef.current(detailId),
    [detailId],
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
          size: 280,
          cell: (info) => (
            <TextCell
              value={
                info.row.original.metadata?.host ||
                info.row.original.metadata?.destinationIP ||
                ''
              }
              search={search}
              primary
            />
          ),
        },
        {
          id: 'Chains',
          header: () => m.connections_column_chains(),
          accessorFn: ({ chains }) => [...chains].reverse().join(' / '),
          size: 320,
          cell: (info) => (
            <ChainCell chains={info.row.original.chains} search={search} />
          ),
        },
        {
          id: 'Downloaded',
          header: () => m.connections_column_downloaded(),
          accessorFn: ({ download }) => parseTraffic(download).join(' '),
          sortFn: (rowA, rowB) =>
            rowA.original.download - rowB.original.download,
          size: 110,
          meta: { align: 'end' },
          cell: (info) => <TrafficCell value={info.row.original.download} />,
        },
        {
          id: 'Uploaded',
          header: () => m.connections_column_uploaded(),
          accessorFn: ({ upload }) => parseTraffic(upload).join(' '),
          sortFn: (rowA, rowB) => rowA.original.upload - rowB.original.upload,
          size: 110,
          meta: { align: 'end' },
          cell: (info) => <TrafficCell value={info.row.original.upload} />,
        },
        {
          id: 'DL Speed',
          header: () => m.connections_column_download_speed(),
          accessorFn: ({ downloadSpeed }) =>
            parseTraffic(downloadSpeed).join(' ') + '/s',
          sortFn: (rowA, rowB) =>
            rowA.original.downloadSpeed - rowB.original.downloadSpeed,
          size: 110,
          meta: { align: 'end' },
          cell: (info) => (
            <TrafficCell value={info.row.original.downloadSpeed} rate />
          ),
        },
        {
          id: 'UL Speed',
          header: () => m.connections_column_upload_speed(),
          accessorFn: ({ uploadSpeed }) =>
            parseTraffic(uploadSpeed).join(' ') + '/s',
          sortFn: (rowA, rowB) =>
            rowA.original.uploadSpeed - rowB.original.uploadSpeed,
          size: 110,
          meta: { align: 'end' },
          cell: (info) => (
            <TrafficCell value={info.row.original.uploadSpeed} rate />
          ),
        },
        {
          id: 'Process',
          header: () => m.connections_column_process(),
          accessorFn: ({ metadata }) => metadata?.process,
          size: 160,
          cell: (info) => (
            <TextCell
              value={info.row.original.metadata?.process || ''}
              search={search}
            />
          ),
        },
        {
          id: 'Rule',
          header: () => m.connections_column_rule(),
          accessorFn: ({ rule, rulePayload }) =>
            rulePayload ? `${rule} (${rulePayload})` : rule,
          size: 200,
          cell: (info) => (
            <RuleCell
              kind={info.row.original.rule}
              payload={info.row.original.rulePayload}
              search={search}
            />
          ),
        },
        {
          id: 'Time',
          header: () => m.connections_column_time(),
          accessorFn: ({ start }) =>
            formatRelativeTime(start, Date.now(), getLocale()),
          sortFn: (rowA, rowB) => rowA.original.startMs - rowB.original.startMs,
          size: 110,
          cell: (info) => <RelativeTimeCell ms={info.row.original.startMs} />,
        },
        {
          id: 'Source',
          header: () => m.connections_column_source(),
          accessorFn: ({ metadata }) =>
            `${metadata?.sourceIP ?? ''}:${metadata?.sourcePort ?? ''}`,
          size: 160,
          cell: (info) => (
            <TextCell
              value={`${info.row.original.metadata?.sourceIP ?? ''}:${info.row.original.metadata?.sourcePort ?? ''}`}
              search={search}
            />
          ),
        },
        {
          id: 'Destination IP',
          header: () => m.connections_column_destination(),
          accessorFn: ({ metadata }) =>
            `${metadata?.destinationIP ?? ''}:${metadata?.destinationPort ?? ''}`,
          size: 160,
          cell: (info) => (
            <TextCell
              value={`${info.row.original.metadata?.destinationIP || ''}:${info.row.original.metadata?.destinationPort || ''}`}
              search={search}
            />
          ),
        },
        {
          id: 'Type',
          header: () => m.connections_column_type(),
          accessorFn: ({ metadata }) =>
            `${metadata?.type ?? ''} (${metadata?.network ?? ''})`,
          size: 120,
          cell: (info) => (
            <TextCell
              value={`${info.row.original.metadata?.type ?? ''} (${info.row.original.metadata?.network ?? ''})`}
              search={search}
            />
          ),
        },
      ] satisfies Array<ConnectionColumn<ConnectionRow>>,
    [search],
  )

  return (
    <>
      <ConnectionsTable
        settingsKey="connections-columns-active"
        columns={columns}
        data={rows}
        getRowId={connectionId}
        renderRow={renderRow}
        isRowEqual={sameTraffic}
        // Matching ids are on their way; nothing is known to be empty yet.
        emptyMessage={
          unavailable
            ? m.connections_active_unavailable()
            : loading
              ? ''
              : m.connections_empty_message()
        }
        settingsOpen={settingsOpen}
        onSettingsOpenChange={onSettingsOpenChange}
      />

      <RowsTickContext.Provider value={connections}>
        <ConnectionDetailModal
          detail={detail}
          onClose={closeDetail}
          onCloseConnection={closeDetailConnection}
        />
      </RowsTickContext.Provider>
    </>
  )
})

export default ActiveViewer
