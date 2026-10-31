import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

/**
 * The config keys and values the selected core reads, for the settings that
 * only some cores support. Follows the core through the application config
 * event.
 */
export const useConfigFeatures = () => {
  const api = useQueryApi()
  return useQuery(
    unwrapQueryOptions(
      api.queries.getConfigFeatures(),
      api.queries.getConfigFeatures().queryFn!,
    ),
  )
}
