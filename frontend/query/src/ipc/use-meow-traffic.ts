import type { MeowTrafficLedgerQuery } from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

type MeowCore = 'meow' | 'meow-alpha'

export function useMeowStatistics(core: MeowCore) {
  const api = useQueryApi()
  const options = api.queries.getMeowStatistics()
  return useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    queryKey: [...options.queryKey, core],
    refetchInterval: 1_000,
    gcTime: 0,
  })
}

export function useMeowTrafficLedger(
  core: MeowCore,
  query: MeowTrafficLedgerQuery,
) {
  const api = useQueryApi()
  const options = api.queries.queryMeowTrafficLedger(query)
  return useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    queryKey: [...options.queryKey, core],
    gcTime: 0,
  })
}
