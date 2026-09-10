import { useQuery } from '@tanstack/react-query'
import { queries } from './bindings'
import { unwrapQueryOptions } from './query-options'

export const useServicePrompt = () => {
  const query = useQuery(
    unwrapQueryOptions(
      queries.getServiceInstallPrompt(),
      queries.getServiceInstallPrompt().queryFn!,
    ),
  )

  return {
    ...query,
  }
}
