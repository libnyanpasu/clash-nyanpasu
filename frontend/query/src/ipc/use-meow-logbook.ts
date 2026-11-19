import type { MeowLogbookQuery } from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import type { MeowCore } from './meow-core'
import { unwrapQueryOptions } from './query-options'

const REFRESH_INTERVAL_MS = 5000

export function useMeowLogbook(core: MeowCore, query: MeowLogbookQuery) {
  const api = useQueryApi()
  const options = api.queries.queryMeowLogbook(query)

  return useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    queryKey: [...options.queryKey, core],
    gcTime: 0,
    // Keep history cursors stable while browsing older pages.
    refetchInterval: query.before === null ? REFRESH_INTERVAL_MS : false,
  })
}
