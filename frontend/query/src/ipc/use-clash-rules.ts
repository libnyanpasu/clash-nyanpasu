import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

export const useClashRules = () => {
  const api = useQueryApi()
  const query = useQuery(
    unwrapQueryOptions(
      api.queries.clashApiGetRules(),
      api.queries.clashApiGetRules().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
