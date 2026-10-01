import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, type GroupBy } from './rpc-bindings'

/**
 * Session traffic of the given groups, in request order; groups without
 * traffic are left out.
 */
export function useTrafficUsageByKeys(groupBy: GroupBy, keys: string[]) {
  return useQuery({
    queryKey: ['traffic-usage-by-keys', groupBy, keys],
    queryFn: async () =>
      unwrapResult(await commands.queryTrafficUsageByKeys(groupBy, keys)),
    enabled: keys.length > 0,
    // New keys (another filter or config) keep showing the previous answer
    // until theirs arrives.
    placeholderData: keepPreviousData,
    refetchInterval: 2000,
    // Unavailable recording fails every time; the next poll retries anyway.
    retry: false,
  })
}
