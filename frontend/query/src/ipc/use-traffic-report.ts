import { unwrapResult } from '@nyanpasu/rpc'
import type { ReportRequest } from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

const sameLayers = (a: ReportRequest, b: ReportRequest) => {
  const left = a.topology?.layers ?? []
  const right = b.topology?.layers ?? []

  return left.length === right.length && left.every((d, i) => d === right[i])
}

/**
 * The total, rankings and topology of the traffic `request.query` selects.
 * `refetchInterval: false` stops the polling and keeps the last report, also
 * when the window regains focus or the network reconnects. `enabled: false`
 * asks for nothing.
 */
export function useTrafficReport(
  request: ReportRequest,
  options?: { refetchInterval?: number | false; enabled?: boolean },
) {
  const api = useQueryApi()
  const refetchInterval = options?.refetchInterval ?? 2000
  const paused = refetchInterval === false

  return useQuery({
    queryKey: ['traffic-report', request] as const,
    queryFn: async () => unwrapResult(await api.queryTrafficReport(request)),
    // Another request keeps showing the previous report until its own arrives,
    // but a topology only fits the columns it was asked for.
    placeholderData: (previous, previousQuery) =>
      previous &&
      previousQuery &&
      (sameLayers(previousQuery.queryKey[1], request)
        ? previous
        : { ...previous, topology: null }),
    refetchInterval,
    enabled: options?.enabled ?? true,
    refetchOnWindowFocus: !paused,
    refetchOnReconnect: !paused,
    // Unavailable recording fails every time; the next poll retries anyway.
    retry: false,
  })
}
