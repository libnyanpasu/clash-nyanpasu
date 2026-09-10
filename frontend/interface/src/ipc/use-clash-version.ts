import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

export const useClashVersion = () => {
  const query = useQuery(
    unwrapQueryOptions(
      queries.clashApiGetVersion(),
      queries.clashApiGetVersion().queryFn!,
    ),
  )

  return query
}
