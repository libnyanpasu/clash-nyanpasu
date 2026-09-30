import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useIsAppImage = () => {
  return useQuery({
    ...unwrapQueryOptions(
      rpc.queries.isAppimage(),
      rpc.queries.isAppimage().queryFn!,
    ),
    staleTime: Infinity,
  })
}
