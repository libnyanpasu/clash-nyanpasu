import { useCallback, useDeferredValue, useMemo, useRef } from 'react'
import { useMockConnectionsNow } from '@/hooks/use-mock-connections'
import { searchableText } from '@/utils/searchable-text'
import { RANGE_HOURS } from '@/utils/traffic-retention'
import {
  useClashConnectionDetails,
  useTrafficActiveConnectionIds,
  useTrafficClosedConnections,
} from '@nyanpasu/query'
import type {
  ClashConnection_Serialize,
  ClosedConnection,
  Dimensions,
  TrafficRange,
} from '@nyanpasu/rpc/types'
import {
  toTrafficFilters,
  type SearchFilter,
} from '../../_modules/traffic-filters'
import { groupKey } from '../../topology/_modules/mock-traffic'
import {
  mockActiveConnections,
  mockActiveDimensions,
  mockClosedConnections,
} from './mock-connections'

export type ConnectionRow = ClashConnection_Serialize & {
  // Parsed once per sample: sorting by time compares numbers instead of
  // parsing both dates in every comparison.
  startMs: number
}

/** The traffic page's selection, as a jump to the connections page brings it. */
export type ConnectionsSelection = {
  /** Applies to closed connections only, as in the traffic report. */
  range?: TrafficRange
  filters: SearchFilter[]
}

export const isFiltered = (selection: ConnectionsSelection) =>
  selection.filters.length > 0 || selection.range !== undefined

// The store keys closed connections by both: a new core reuses ids.
export const closedConnectionId = (row: ClosedConnection) =>
  `${row.closed_at}:${row.id}`

// Dev builds only: the generated connections filtered the way the backend's
// matcher would, closely enough to preview the page.
const matchesMock = (dimensions: Dimensions, filters: SearchFilter[]) =>
  filters.every(({ d, v }) => groupKey(dimensions, d) === v)

export const mockActiveRows = (now: number, filters: SearchFilter[]) =>
  mockActiveConnections(now).filter((conn) =>
    matchesMock(mockActiveDimensions(conn), filters),
  )

export function mockClosedRows(now: number, selection: ConnectionsSelection) {
  const { range, filters } = selection
  const since =
    range === undefined || range === 'all'
      ? -Infinity
      : now - RANGE_HOURS[range] * 60 * 60 * 1000

  return mockClosedConnections(now).filter(
    (conn) => conn.closed_at >= since && matchesMock(conn.dimensions, filters),
  )
}

/**
 * Keeps the rows that go through `proxy` and contain `search`. A row's
 * searchable text never changes, so it is built once and kept by id in
 * `cache` while the row stays.
 */
function filterRows<TRow>(
  rows: TRow[],
  { search, proxy }: { search: string; proxy?: string | null },
  chainsOf: (row: TRow) => string[],
  idOf: (row: TRow) => string,
  cache: { current: Map<string, string> },
) {
  const byProxy = rows.filter((row) =>
    proxy ? chainsOf(row).includes(proxy) : true,
  )

  if (!search) {
    return byProxy
  }

  const term = search.toLowerCase()
  const previous = cache.current
  const texts = new Map<string, string>()

  const matched = byProxy.filter((row) => {
    const id = idOf(row)
    const text = previous.get(id) ?? searchableText(row)
    texts.set(id, text)
    return text.includes(term)
  })

  cache.current = texts

  return matched
}

const activeChains = (row: ConnectionRow) => row.chains ?? []

const activeId = (row: ConnectionRow) => row.id

const closedChains = (row: ClosedConnection) => row.dimensions.chains

/**
 * The live connections. With `filters`, only those the traffic history
 * matches: none until it says which, so a filtered view never shows all.
 */
export function useActiveConnectionRows({
  search,
  proxy,
  filters,
}: {
  search: string
  proxy?: string | null
  filters: SearchFilter[]
}) {
  const { data: latest } = useClashConnectionDetails()

  // Rebuilding the table from a sample is the bulk of every frame. A deferred
  // sample renders in the background, where input and scrolling interrupt it.
  const details = useDeferredValue(latest)

  const mockNow = useMockConnectionsNow()

  const filtered = filters.length > 0

  const ids = useTrafficActiveConnectionIds(toTrafficFilters(filters), {
    enabled: filtered && mockNow === null,
  })

  const matchedIds = useMemo(
    () => (ids.isError || !ids.data ? undefined : new Set(ids.data)),
    [ids.isError, ids.data],
  )

  const connections = useMemo<ConnectionRow[]>(() => {
    const live =
      mockNow === null
        ? (details?.connections ?? []).filter(
            (conn) => !filtered || matchedIds?.has(conn.id),
          )
        : mockActiveRows(mockNow, filters)

    return live.map((conn) => ({ ...conn, startMs: Date.parse(conn.start) }))
  }, [details, mockNow, filters, filtered, matchedIds])

  const searchTexts = useRef(new Map<string, string>())

  const rows = useMemo(
    () =>
      filterRows(
        connections,
        { search, proxy },
        activeChains,
        activeId,
        searchTexts,
      ),
    [connections, search, proxy],
  )

  const loading =
    filtered && mockNow === null && matchedIds === undefined && !ids.isError

  // Without the history nothing is known to match: not an empty result.
  const unavailable = filtered && mockNow === null && ids.isError

  return { connections, rows, loading, unavailable }
}

/** The closed connections of the selection, a page at a time. */
export function useClosedConnectionRows({
  search,
  proxy,
  selection,
}: {
  search: string
  proxy?: string | null
  selection: ConnectionsSelection
}) {
  const {
    data: history,
    error,
    hasNextPage,
    isFetchingNextPage,
    isFetchNextPageError,
    fetchNextPage,
  } = useTrafficClosedConnections({
    range: selection.range ?? 'all',
    filters: toTrafficFilters(selection.filters),
  })

  const mockNow = useMockConnectionsNow()

  // A poll that brings new records shifts the pages, which hands out the
  // others as new objects; the cache keys their text by row id instead.
  const searchTexts = useRef(new Map<string, string>())

  const rows = useMemo(
    () =>
      filterRows(
        mockNow === null
          ? (history?.pages ?? []).flatMap((page) => page.connections)
          : mockClosedRows(mockNow, selection),
        { search, proxy },
        closedChains,
        closedConnectionId,
        searchTexts,
      ),
    [history, mockNow, selection, search, proxy],
  )

  // A new function whenever a fetch starts or ends, so an empty table asks
  // again: a page may hold no match yet still lead to older ones.
  const onEndReached = useCallback(() => {
    // A failed page waits for the next poll instead of retrying in a loop.
    if (hasNextPage && !isFetchingNextPage && !isFetchNextPageError) {
      fetchNextPage()
    }
  }, [hasNextPage, isFetchingNextPage, isFetchNextPageError, fetchNextPage])

  return { rows, error, hasNextPage, onEndReached }
}
