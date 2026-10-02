import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

export const useServicePrompt = () => {
  const api = useQueryApi()
  const query = useQuery(
    unwrapQueryOptions(
      api.queries.getServiceInstallPrompt(),
      api.queries.getServiceInstallPrompt().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
