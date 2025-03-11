import { useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands } from './bindings'
import { NYANPASU_SYSTEM_PROXY_QUERY_KEY } from './consts'

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
    queryKey: [NYANPASU_SYSTEM_PROXY_QUERY_KEY],
    queryFn: async () => {
      return unwrapResult(await commands.getSysProxy())
    },
  })

  return {
    ...query,
  }
}
