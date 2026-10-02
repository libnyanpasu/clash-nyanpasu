import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

export const useIsAppImage = () => {
  const api = useQueryApi()
  return useQuery({
    ...unwrapQueryOptions(
      api.queries.isAppimage(),
      api.queries.isAppimage().queryFn!,
    ),
    staleTime: Infinity,
  })
}

export const useIsPortable = () => {
  const api = useQueryApi()
  return useQuery({
    ...unwrapQueryOptions(
      api.queries.isPortable(),
      api.queries.isPortable().queryFn!,
    ),
    staleTime: Infinity,
  })
}
