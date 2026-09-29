import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useClashVersion = () => {
  const query = useQuery(
    unwrapQueryOptions(
      rpc.queries.clashApiGetVersion(),
      rpc.queries.clashApiGetVersion().queryFn!,
    ),
  )

  return query
}
