import { unwrapResult } from '@nyanpasu/rpc'
import type { TrafficFilter } from '@nyanpasu/rpc/types'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

/**
 * The ids of the live connections that satisfy every filter, sorted.
 * `enabled: false` asks for nothing.
 */
export function useTrafficActiveConnectionIds(
  filters: TrafficFilter[],
  options?: { enabled?: boolean },
) {
  const api = useQueryApi()

  return useQuery({
    queryKey: ['traffic-active-connection-ids', filters] as const,
    queryFn: async () =>
      unwrapResult(await api.queryTrafficActiveConnectionIds(filters)),
    // Other filters keep showing the previous ids until their own arrive.
    placeholderData: keepPreviousData,
    refetchInterval: 1000,
    enabled: options?.enabled ?? true,
    // Unavailable recording fails every time; the next poll retries anyway.
    retry: false,
  })
}
