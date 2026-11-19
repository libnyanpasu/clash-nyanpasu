import type { MeowLogbookQuery } from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import type { MeowCore } from './meow-core'

const REFRESH_INTERVAL_MS = 5000

export function useMeowLogbook(core: MeowCore, query: MeowLogbookQuery) {
  const api = useQueryApi()
  return useQuery({
    queryKey: ['queryMeowLogbook', query, core],
    queryFn: async () => await api.queryMeowLogbook(query),
    gcTime: 0,
    // Keep history cursors stable while browsing older pages.
    refetchInterval: query.before === null ? REFRESH_INTERVAL_MS : false,
  })
}
