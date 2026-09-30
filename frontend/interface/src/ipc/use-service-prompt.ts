import { useQuery } from '@tanstack/react-query'
import { unwrapQueryOptions } from './query-options'
import { rpc } from './rpc'

export const useServicePrompt = () => {
  const query = useQuery(
    unwrapQueryOptions(
      rpc.queries.getServiceInstallPrompt(),
      rpc.queries.getServiceInstallPrompt().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
