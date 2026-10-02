import { unwrapResult } from '@nyanpasu/rpc'
import type { ClosedCursor } from '@nyanpasu/rpc/types'
import { useInfiniteQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

const PAGE_SIZE = 200

const POLL_INTERVAL = 2000

/** Closed connections of the current traffic session, newest first. */
export function useTrafficClosedConnections() {
  const api = useQueryApi()
  return useInfiniteQuery({
    queryKey: ['traffic-closed-connections'],
    initialPageParam: null as ClosedCursor | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(
        await api.queryTrafficClosedConnections(pageParam, PAGE_SIZE),
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
