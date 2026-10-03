import { unwrapResult } from '@nyanpasu/rpc'
import type {
  ClosedCursor,
  TrafficFilter,
  TrafficRange,
} from '@nyanpasu/rpc/types'
import { useInfiniteQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

const PAGE_SIZE = 200

const POLL_INTERVAL = 2000

/** The closed connections a traffic report over `range` and `filters` counts. */
export type ClosedConnectionsSelection = {
  range: TrafficRange
  filters: TrafficFilter[]
}

/**
 * Closed connections `selection` picks, newest first. A page may hold fewer
 * rows than asked for, even none, and still continue.
 */
export function useTrafficClosedConnections(
  selection: ClosedConnectionsSelection,
) {
  const api = useQueryApi()

  return useInfiniteQuery({
    queryKey: ['traffic-closed-connections', selection] as const,
    initialPageParam: null as ClosedCursor | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(
        await api.queryTrafficClosedConnections(
          selection.range,
          selection.filters,
          pageParam,
          PAGE_SIZE,
        ),
      ),
    getNextPageParam: (page) => page.next ?? undefined,
    // A poll refetches every loaded page, so it slows down as more are loaded
    // to keep one page read per interval; only the newest page gains rows.
    refetchInterval: (query) =>
      POLL_INTERVAL * Math.max(1, query.state.data?.pages.length ?? 1),
    // Unavailable recording fails every time; the next poll retries anyway.
    retry: false,
  })
}
