import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useClashRules = () => {
  const query = useQuery(
    unwrapQueryOptions(
      rpc.queries.clashApiGetRules(),
      rpc.queries.clashApiGetRules().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
