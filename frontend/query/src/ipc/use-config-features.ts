import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

/**
 * The config keys and values the selected core reads, for the settings that
 * only some cores support. Follows the core through the application config
 * event.
 */
export const useConfigFeatures = () => {
  const api = useQueryApi()
  return useQuery(api.queries.getConfigFeatures())
}
