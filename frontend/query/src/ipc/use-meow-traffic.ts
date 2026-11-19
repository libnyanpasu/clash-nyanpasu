import type { MeowTrafficLedgerQuery } from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import type { MeowCore } from './meow-core'

export function useMeowStatistics(core: MeowCore) {
  const api = useQueryApi()
  return useQuery({
    queryKey: ['getMeowStatistics', core],
    queryFn: async () => await api.getMeowStatistics(),
    refetchInterval: 1_000,
    gcTime: 0,
  })
}

export function useMeowTrafficLedger(
  core: MeowCore,
  query: MeowTrafficLedgerQuery,
) {
  const api = useQueryApi()
  return useQuery({
    queryKey: ['queryMeowTrafficLedger', query, core],
    queryFn: async () => await api.queryMeowTrafficLedger(query),
    gcTime: 0,
  })
}
