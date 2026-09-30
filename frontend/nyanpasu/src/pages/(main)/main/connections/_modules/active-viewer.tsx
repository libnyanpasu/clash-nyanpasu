import dayjs from 'dayjs'
import { memo, useMemo, useState } from 'react'
import { m } from '@/paraglide/messages'
import { containsSearchTerm } from '@/utils'
import parseTraffic from '@/utils/parse-traffic'
import {
  ClashConnection_Serialize,
  useClashConnectionDetails,
} from '@nyanpasu/interface'
import { ChainCell, RuleCell, TextCell, TrafficCell } from './cells'
import ConnectionsTable, { type ConnectionColumn } from './connections-table'
import TableRow, { ConnectionDetailModal } from './table-row'

export type ConnectionRow = ClashConnection_Serialize & {
  // Parsed once per sample: sorting by time compares numbers instead of
  // parsing both dates in every comparison.
  startMs: number
}

// Memoized so a keystroke's urgent render skips the table; it re-renders
// with the deferred search term, or on its own stream and prop updates.
const ActiveViewer = memo(function ActiveViewer({
  search,
  proxy,
  settingsOpen,
  onSettingsOpenChange,
}: {
  search: string
  proxy?: string | null
  settingsOpen: boolean
  onSettingsOpenChange: (open: boolean) => void
}) {
  const { data: details } = useClashConnectionDetails()

  const data = useMemo<ConnectionRow[]>(() => {
    const connections = details?.connections ?? []

    return connections
      .filter((conn) => (proxy ? conn.chains?.includes(proxy) : true))
      .map((conn) => ({
        ...conn,
        startMs: Date.parse(conn.start),
      }))
      .filter((c) => (search ? containsSearchTerm(c, search) : true))
  }, [details, search, proxy])

  const [detailId, setDetailId] = useState<string | null>(null)

  const detailRow = useMemo(
    () =>
      detailId === null ? undefined : data.find((row) => row.id === detailId),
    [data, detailId],
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
          accessorFn: ({ start }) => dayjs(start).fromNow(),
          sortFn: (rowA, rowB) => rowA.original.startMs - rowB.original.startMs,
          size: 110,
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
        data={data}
        getRowId={(row) => row.id}
        renderRow={(row, props) => (
          <TableRow {...props} data={row} onViewDetails={setDetailId} />
        )}
        emptyMessage={m.connections_empty_message()}
        settingsOpen={settingsOpen}
        onSettingsOpenChange={onSettingsOpenChange}
      />

      <ConnectionDetailModal
        data={detailRow}
        onClose={() => setDetailId(null)}
      />
    </>
  )
})

export default ActiveViewer
