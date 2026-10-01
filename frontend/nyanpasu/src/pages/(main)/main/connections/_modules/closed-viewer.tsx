import { memo, useCallback, useMemo, useRef, useState } from 'react'
import { useMockConnectionsNow } from '@/hooks/use-mock-connections'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatRelativeTime } from '@/utils/date'
import parseTraffic from '@/utils/parse-traffic'
import { searchableText } from '@/utils/searchable-text'
import {
  useTrafficClosedConnections,
  type ClosedConnection,
} from '@nyanpasu/interface'
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
import { mockClosedConnections } from './mock-connections'
import TableRow, {
  closedConnectionDetail,
  ConnectionDetailModal,
} from './table-row'

// The store keys closed connections by both: a new core reuses ids.
const closedConnectionId = (row: ClosedConnection) =>
  `${row.closed_at}:${row.id}`

// A closed record never changes, so a row keyed by it always shows the same.
const sameRecord = () => true

// The traffic history keeps the process path; the table shows its name.
const processName = (process: string) => process.split('/').pop() || process

// Closed connections of the current traffic session. Memoized like the active
// view, so a keystroke's urgent render skips the table.
const ClosedViewer = memo(function ClosedViewer({
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
  const {
    data: history,
    error,
    hasNextPage,
    isFetchingNextPage,
    isFetchNextPageError,
    fetchNextPage,
  } = useTrafficClosedConnections()

  const mockNow = useMockConnectionsNow()

  // A closed record never changes, so its searchable text is built once and
  // kept by row id: a poll that brings new records shifts the pages, which
  // hands out the others as new objects.
  const searchTexts = useRef(new Map<string, string>())

  const data = useMemo(() => {
    const byProxy = (
      mockNow === null
        ? (history?.pages ?? []).flatMap((page) => page.connections)
        : mockClosedConnections(mockNow)
    ).filter((conn) => (proxy ? conn.dimensions.chains.includes(proxy) : true))

    if (!search) {
      return byProxy
    }

    const term = search.toLowerCase()
    const previous = searchTexts.current
    const texts = new Map<string, string>()

    const matched = byProxy.filter((conn) => {
      const id = closedConnectionId(conn)
      const text = previous.get(id) ?? searchableText(conn)
      texts.set(id, text)
      return text.includes(term)
    })

    searchTexts.current = texts

    return matched
  }, [history, mockNow, search, proxy])

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

  const handleEndReached = useCallback(() => {
    // A failed page waits for the next poll instead of retrying in a loop.
    if (hasNextPage && !isFetchingNextPage && !isFetchNextPageError) {
      fetchNextPage()
    }
  }, [hasNextPage, isFetchingNextPage, isFetchNextPageError, fetchNextPage])

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
        data={data}
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
        onEndReached={handleEndReached}
        settingsOpen={settingsOpen}
        onSettingsOpenChange={onSettingsOpenChange}
      />

      <RowsTickContext.Provider value={data}>
        <ConnectionDetailModal detail={detail} onClose={closeDetail} />
      </RowsTickContext.Provider>
    </>
  )
})

export default ClosedViewer
