import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

export const useIsAppImage = () => {
  return useQuery({
    ...unwrapQueryOptions(queries.isAppimage(), queries.isAppimage().queryFn!),
    staleTime: Infinity,
  })
}
