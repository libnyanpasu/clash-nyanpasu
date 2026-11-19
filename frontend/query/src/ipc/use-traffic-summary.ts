import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

/** The current traffic session: totals, connection counts and rate. */
export function useTrafficSummary() {
  const api = useQueryApi()
  return useQuery({
    queryKey: ['traffic-summary'],
    queryFn: async () => await api.getTrafficSummary(),
    refetchInterval: 2000,
    // Unavailable recording fails every time; the next poll retries anyway.
    retry: false,
  })
}
