import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

export const useCoreDir = () => {
  const query = useQuery(
    unwrapQueryOptions(queries.getCoreDir(), queries.getCoreDir().queryFn!),
  )

  return {
    ...query,
  }
}
