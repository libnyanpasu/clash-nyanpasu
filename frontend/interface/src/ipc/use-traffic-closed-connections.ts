import { useInfiniteQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, type ClosedCursor } from './bindings'

const PAGE_SIZE = 200

const POLL_INTERVAL = 2000

/** Closed connections of the current traffic session, newest first. */
export function useTrafficClosedConnections() {
  return useInfiniteQuery({
    queryKey: ['traffic-closed-connections'],
    initialPageParam: null as ClosedCursor | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(
        await commands.queryTrafficClosedConnections(pageParam, PAGE_SIZE),
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
