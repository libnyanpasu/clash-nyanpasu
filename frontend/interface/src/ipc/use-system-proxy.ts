import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands } from './bindings'

/**
 * Custom hook to fetch and manage the system proxy settings.
 *
 * This hook leverages the `useQuery` hook to perform an asynchronous request
 * to obtain system proxy data via `commands.getSysProxy()`. The result of the query
 * is processed with `unwrapResult` to extract the proxy information.
 *
 * @returns An object containing the query results and helper properties/methods
 *          (e.g., loading status, error, and refetch function) provided by `useQuery`.
 */
export const useSystemProxy = () => {
  const query = useQuery({
    queryKey: ['system-proxy'],
    queryFn: async () => {
      return unwrapResult(await commands.getSysProxy())
    },
  })

  return {
    ...query,
  }
}
