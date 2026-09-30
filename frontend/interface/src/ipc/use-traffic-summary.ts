import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands } from './bindings'

/** The current traffic session: totals, connection counts and rate. */
export function useTrafficSummary() {
  return useQuery({
    queryKey: ['traffic-summary'],
    queryFn: async () => unwrapResult(await commands.getTrafficSummary()),
    refetchInterval: 2000,
    // Unavailable recording fails every time; the next poll retries anyway.
    retry: false,
  })
}
