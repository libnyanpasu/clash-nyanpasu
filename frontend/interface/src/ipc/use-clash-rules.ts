import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

export const useClashRules = () => {
  const query = useQuery(
    unwrapQueryOptions(
      queries.clashApiGetRules(),
      queries.clashApiGetRules().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
