import { useCallback, useMemo } from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import type { AuditCursor, AuditFilter, AuditPage } from '@nyanpasu/rpc/types'
import {
  useInfiniteQuery,
  useQueryClient,
  type InfiniteData,
} from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

const PAGE_SIZE = 200

/**
 * Persisted HTTP and local RPC audit records matching `filter`, newest first.
 * The first page loads on mount; nothing polls. `fetchNextPage` loads older
 * records and `refresh` starts over from the newest page.
 */
export function useRpcAuditLog(filter: AuditFilter) {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const queryKey = useMemo(() => ['rpc-audit-log', filter] as const, [filter])

  const query = useInfiniteQuery({
    queryKey,
    initialPageParam: null as AuditCursor | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(await api.queryRpcAudit(filter, pageParam, PAGE_SIZE)),
    getNextPageParam: (page) => page.next ?? undefined,
    // Records change only through a refresh the user asks for.
    refetchOnWindowFocus: false,
    // Unavailable storage fails every time until the app restarts.
    retry: false,
  })

  const { refetch } = query
  const refresh = useCallback(async () => {
    // Refetching keeps every loaded page; drop the older ones first so the
    // refresh reads only the newest page.
    queryClient.setQueryData<InfiniteData<AuditPage, AuditCursor | null>>(
      queryKey,
      (data) =>
        data && {
          pages: data.pages.slice(0, 1),
          pageParams: data.pageParams.slice(0, 1),
        },
    )
    await refetch()
  }, [queryClient, queryKey, refetch])

  return { query, refresh }
}
