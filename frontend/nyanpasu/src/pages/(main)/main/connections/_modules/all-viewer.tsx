import { memo, useCallback, useMemo, useRef, useState } from 'react'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatRelativeTime } from '@/utils/date'
import parseTraffic from '@/utils/parse-traffic'
import { useDeleteClashConnections } from '@nyanpasu/query'
import type { ClosedConnection } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
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
  type ConnectionDetail,
} from './table-row'
import {
  closedConnectionId,
  processName,
  sameTraffic,
  useActiveConnectionDetail,
  useActiveConnectionRows,
  useClosedConnectionRows,
  type ConnectionRow,
  type ConnectionsSelection,
} from './use-connection-rows'

export type AnyConnectionRow =
  | { kind: 'active'; key: string; active: ConnectionRow }
  | { kind: 'closed'; key: string; closed: ClosedConnection }

/**
 * Live rows first in stream order, then closed rows newest first, the order
 * their pages arrive in; a closed row whose id is still live is left out.
 * `live` is every live connection, before search and proxy filtering: a
 * search that hides a live connection must not show its id as closed.
 */
export function mergeConnectionRows(
  active: ConnectionRow[],
  closed: ClosedConnection[],
  live: ConnectionRow[] = active,
): AnyConnectionRow[] {
  const liveIds = new Set(live.map((row) => row.id))

  return [
    ...active.map((row): AnyConnectionRow => ({
      kind: 'active',
      key: `a:${row.id}`,
      active: row,
    })),
    ...closed
      .filter((row) => !liveIds.has(row.id))
      .map((row): AnyConnectionRow => ({
        kind: 'closed',
        key: `c:${closedConnectionId(row)}`,
        closed: row,
      })),
  ]
}

const rowKey = (row: AnyConnectionRow) => row.key

// Live rows change with their traffic; a closed record never changes.
const sameRow = (a: AnyConnectionRow, b: AnyConnectionRow) =>
  a.kind === 'active'
    ? b.kind === 'active' && sameTraffic(a.active, b.active)
    : b.kind === 'closed'

// The fields both kinds share, read from where each kind keeps them.
const hostOf = (row: AnyConnectionRow) =>
  row.kind === 'active'
    ? row.active.metadata?.host || row.active.metadata?.destinationIP || ''
    : row.closed.dimensions.target

const chainsOf = (row: AnyConnectionRow) =>
  row.kind === 'active' ? row.active.chains : row.closed.dimensions.chains

const downloadOf = (row: AnyConnectionRow) =>
  row.kind === 'active' ? row.active.download : row.closed.bytes.download

const uploadOf = (row: AnyConnectionRow) =>
  row.kind === 'active' ? row.active.upload : row.closed.bytes.upload

const processOf = (row: AnyConnectionRow) =>
  row.kind === 'active'
    ? row.active.metadata?.process || ''
    : processName(row.closed.dimensions.process)

const ruleOf = (row: AnyConnectionRow) =>
  row.kind === 'active'
    ? { kind: row.active.rule, payload: row.active.rulePayload }
    : row.closed.dimensions.rule

const startOf = (row: AnyConnectionRow) =>
  row.kind === 'active' ? row.active.startMs : row.closed.started_at

const sourceOf = (row: AnyConnectionRow) =>
  row.kind === 'active'
    ? `${row.active.metadata?.sourceIP ?? ''}:${row.active.metadata?.sourcePort ?? ''}`
    : row.closed.dimensions.source

const typeOf = (row: AnyConnectionRow) =>
  row.kind === 'active'
    ? `${row.active.metadata?.type ?? ''} (${row.active.metadata?.network ?? ''})`
    : row.closed.dimensions.protocol

// Only one kind has these; the other's cell stays empty and sorts last.
const downloadSpeedOf = (row: AnyConnectionRow) =>
  row.kind === 'active' ? row.active.downloadSpeed : undefined

const uploadSpeedOf = (row: AnyConnectionRow) =>
  row.kind === 'active' ? row.active.uploadSpeed : undefined

const destinationOf = (row: AnyConnectionRow) =>
  row.kind === 'active'
    ? `${row.active.metadata?.destinationIP || ''}:${row.active.metadata?.destinationPort || ''}`
    : undefined

const closedAtOf = (row: AnyConnectionRow) =>
  row.kind === 'closed' ? row.closed.closed_at : undefined

// Only called on two present values: `sortUndefined` orders the empty ones.
const byNumber =
  (valueOf: (row: AnyConnectionRow) => number | undefined) =>
  (
    rowA: { original: AnyConnectionRow },
    rowB: { original: AnyConnectionRow },
  ) =>
    (valueOf(rowA.original) ?? 0) - (valueOf(rowB.original) ?? 0)

// `key` is the row's id in this table, which the details report back.
type DetailTarget =
  | { kind: 'active'; key: string; id: string }
  | { kind: 'closed'; key: string; row: ClosedConnection }

// Live and closed connections of the selection in one table, as the traffic
// page's "all" scope counts them. Memoized like the other views, so a
// keystroke's urgent render skips the table.
const AllViewer = memo(function AllViewer({
  search,
  proxy,
  selection,
  settingsOpen,
  onSettingsOpenChange,
  onLocateRule,
  onViewRuleUsage,
  focusRowId,
}: {
  search: string
  proxy?: string | null
  selection: ConnectionsSelection
  settingsOpen: boolean
  onSettingsOpenChange: (open: boolean) => void
  onLocateRule?: (detail: ConnectionDetail) => void
  onViewRuleUsage?: (detail: ConnectionDetail) => void
  // The row to scroll to and highlight once it appears.
  focusRowId?: string
}) {
  const { connections, rows: activeRows } = useActiveConnectionRows({
    search,
    proxy,
    filters: selection.filters,
  })

  const {
    rows: closedRows,
    error,
    hasNextPage,
    onEndReached,
  } = useClosedConnectionRows({ search, proxy, selection })

  const rows = useMemo(
    () => mergeConnectionRows(activeRows, closedRows, connections),
    [activeRows, closedRows, connections],
  )

  const deleteConnections = useDeleteClashConnections()

  // `mutateAsync` is a new function on every render; the row renderer stays
  // the same and calls the latest one, so rows can skip re-rendering.
  const deleteConnectionsRef = useRef(deleteConnections.mutateAsync)
  deleteConnectionsRef.current = deleteConnections.mutateAsync

  const [detailTarget, setDetailTarget] = useState<DetailTarget | null>(null)

  const activeDetail = useActiveConnectionDetail(
    connections,
    detailTarget?.kind === 'active' ? detailTarget.id : null,
    detailTarget?.kind === 'active' ? detailTarget.key : null,
  )

  const detail = useMemo(
    () =>
      detailTarget?.kind === 'closed'
        ? closedConnectionDetail(detailTarget.row, detailTarget.key)
        : activeDetail,
    [detailTarget, activeDetail],
  )

  const closeDetail = useCallback(() => setDetailTarget(null), [])

  const closeDetailConnection = useMemo(() => {
    if (detailTarget?.kind !== 'active') {
      return undefined
    }

    const { id } = detailTarget

    return () => deleteConnectionsRef.current(id)
  }, [detailTarget])

  const renderRow = useCallback(
    (row: AnyConnectionRow, props: RowProps) =>
      row.kind === 'active' ? (
        <TableRow
          {...props}
          onViewDetails={() =>
            setDetailTarget({
              kind: 'active',
              key: row.key,
              id: row.active.id,
            })
          }
          onCloseConnection={() => deleteConnectionsRef.current(row.active.id)}
        />
      ) : (
        <TableRow
          {...props}
          onViewDetails={() =>
            setDetailTarget({ kind: 'closed', key: row.key, row: row.closed })
          }
        />
      ),
    [],
  )

  const columns = useMemo(
    () =>
      [
        {
          id: 'Status',
          header: () => m.connections_column_status(),
          accessorFn: ({ kind }) => kind,
          size: 72,
          cell: (info) => {
            const live = info.row.original.kind === 'active'

            const status = live
              ? m.connections_tab_active()
              : m.connections_tab_closed()

            return (
              <span
                className={cn(
                  'inline-block size-2 rounded-full',
                  live ? 'bg-primary' : 'bg-outline',
                )}
                data-slot="connections-status-dot"
                role="img"
                aria-label={status}
                title={status}
              />
            )
          },
        },
        {
          // ids match the other tables' columns so all share column sizing
          id: 'Host',
          header: () => m.connections_column_host(),
          accessorFn: hostOf,
          size: 280,
          cell: (info) => (
            <TextCell
              value={hostOf(info.row.original)}
              search={search}
              primary
            />
          ),
        },
        {
          id: 'Chains',
          header: () => m.connections_column_chains(),
          accessorFn: (row) => [...chainsOf(row)].reverse().join(' / '),
          size: 320,
          cell: (info) => (
            <ChainCell chains={chainsOf(info.row.original)} search={search} />
          ),
        },
        {
          id: 'Downloaded',
          header: () => m.connections_column_downloaded(),
          accessorFn: (row) => parseTraffic(downloadOf(row)).join(' '),
          sortFn: byNumber(downloadOf),
          size: 110,
          meta: { align: 'end' },
          cell: (info) => <TrafficCell value={downloadOf(info.row.original)} />,
        },
        {
          id: 'Uploaded',
          header: () => m.connections_column_uploaded(),
          accessorFn: (row) => parseTraffic(uploadOf(row)).join(' '),
          sortFn: byNumber(uploadOf),
          size: 110,
          meta: { align: 'end' },
          cell: (info) => <TrafficCell value={uploadOf(info.row.original)} />,
        },
        {
          id: 'DL Speed',
          header: () => m.connections_column_download_speed(),
          accessorFn: downloadSpeedOf,
          sortFn: byNumber(downloadSpeedOf),
          sortUndefined: 'last',
          size: 110,
          meta: { align: 'end' },
          cell: (info) => {
            const speed = downloadSpeedOf(info.row.original)

            return speed === undefined ? null : (
              <TrafficCell value={speed} rate />
            )
          },
        },
        {
          id: 'UL Speed',
          header: () => m.connections_column_upload_speed(),
          accessorFn: uploadSpeedOf,
          sortFn: byNumber(uploadSpeedOf),
          sortUndefined: 'last',
          size: 110,
          meta: { align: 'end' },
          cell: (info) => {
            const speed = uploadSpeedOf(info.row.original)

            return speed === undefined ? null : (
              <TrafficCell value={speed} rate />
            )
          },
        },
        {
          id: 'Process',
          header: () => m.connections_column_process(),
          accessorFn: processOf,
          size: 160,
          cell: (info) => (
            <span
              title={
                info.row.original.kind === 'closed'
                  ? info.row.original.closed.dimensions.process
                  : undefined
              }
            >
              <TextCell value={processOf(info.row.original)} search={search} />
            </span>
          ),
        },
        {
          id: 'Rule',
          header: () => m.connections_column_rule(),
          accessorFn: (row) => {
            const { kind, payload } = ruleOf(row)

            return payload ? `${kind} (${payload})` : kind
          },
          size: 200,
          cell: (info) => (
            <RuleCell {...ruleOf(info.row.original)} search={search} />
          ),
        },
        {
          id: 'Time',
          header: () => m.connections_field_start(),
          accessorFn: (row) =>
            formatRelativeTime(startOf(row), Date.now(), getLocale()),
          sortFn: byNumber(startOf),
          size: 110,
          cell: (info) => <RelativeTimeCell ms={startOf(info.row.original)} />,
        },
        {
          id: 'Closed',
          header: () => m.connections_column_closed_time(),
          accessorFn: closedAtOf,
          sortFn: byNumber(closedAtOf),
          sortUndefined: 'last',
          size: 110,
          cell: (info) => {
            const closedAt = closedAtOf(info.row.original)

            return closedAt === undefined ? null : (
              <RelativeTimeCell ms={closedAt} />
            )
          },
        },
        {
          id: 'Source',
          header: () => m.connections_column_source(),
          accessorFn: sourceOf,
          size: 160,
          cell: (info) => (
            <TextCell value={sourceOf(info.row.original)} search={search} />
          ),
        },
        {
          id: 'Destination IP',
          header: () => m.connections_column_destination(),
          accessorFn: destinationOf,
          sortUndefined: 'last',
          size: 160,
          cell: (info) => {
            const destination = destinationOf(info.row.original)

            return destination === undefined ? null : (
              <TextCell value={destination} search={search} />
            )
          },
        },
        {
          id: 'Type',
          header: () => m.connections_column_type(),
          accessorFn: typeOf,
          size: 120,
          cell: (info) => (
            <TextCell value={typeOf(info.row.original)} search={search} />
          ),
        },
      ] satisfies Array<ConnectionColumn<AnyConnectionRow>>,
    [search],
  )

  return (
    <>
      <ConnectionsTable
        settingsKey="connections-columns-all"
        columns={columns}
        data={rows}
        getRowId={rowKey}
        renderRow={renderRow}
        isRowEqual={sameRow}
        emptyMessage={
          error
            ? m.connections_closed_unavailable()
            : hasNextPage
              ? m.connections_closed_loading()
              : m.connections_empty_message()
        }
        onEndReached={onEndReached}
        focusRowId={focusRowId}
        settingsOpen={settingsOpen}
        onSettingsOpenChange={onSettingsOpenChange}
      />

      <RowsTickContext.Provider value={connections}>
        <ConnectionDetailModal
          detail={detail}
          onClose={closeDetail}
          onCloseConnection={closeDetailConnection}
          onLocateRule={onLocateRule}
          onViewRuleUsage={onViewRuleUsage}
        />
      </RowsTickContext.Provider>
    </>
  )
})

export default AllViewer
