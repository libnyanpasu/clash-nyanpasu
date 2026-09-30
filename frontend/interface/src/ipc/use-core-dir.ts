import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useCoreDir = () => {
  const query = useQuery(
    unwrapQueryOptions(
      rpc.queries.getCoreDir(),
      rpc.queries.getCoreDir().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
