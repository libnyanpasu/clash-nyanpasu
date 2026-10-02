import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

export const useCoreDir = () => {
  const api = useQueryApi()
  const query = useQuery(
    unwrapQueryOptions(
      api.queries.getCoreDir(),
      api.queries.getCoreDir().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
