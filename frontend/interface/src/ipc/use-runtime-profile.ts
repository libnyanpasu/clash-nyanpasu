import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands } from './bindings'

/**
 * Custom hook for retrieving the runtime profile.
 *
 * This hook leverages the useQuery API to asynchronously retrieve and unwrap the runtime's YAML profile data
 * via the commands.getRuntimeYaml call. The resulting query object includes properties such as data, error,
 * status, and other metadata necessary to manage the loading state.
 *
 * @returns An object containing the query state and helper methods related to the runtime profile.
 */
export const useRuntimeProfile = () => {
  const query = useQuery({
    queryKey: ['runtime-profile'],
    queryFn: async () => {
      return unwrapResult(await commands.getRuntimeYaml())
    },
  })

  return {
    ...query,
  }
}
