import useUpdateEffect from 'react-use/esm/useUpdateEffect'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'
import { useSetting } from './use-settings'

/**
 * Custom hook to fetch and manage the system proxy settings.
 *
 * This hook leverages the `useQuery` hook to perform an asynchronous request
 * to obtain system proxy data via `api.getSysProxy()`. The result of the query
 * is processed with `unwrapResult` to extract the proxy information.
 *
 * Polls while the window is visible: other applications can change it.
 *
 * @returns The system proxy as `data`. Only `data` is exposed so a poll that
 *          finds nothing new does not re-render the caller.
 */
export const useSystemProxy = () => {
  const api = useQueryApi()
  const query = useQuery({
    ...unwrapQueryOptions(
      api.queries.getSysProxy(),
      api.queries.getSysProxy().queryFn!,
    ),
    refetchInterval: 5000,
  })

  const { value } = useSetting('enable_system_proxy')

  useUpdateEffect(() => {
    query.refetch()
  }, [value])

  return { data: query.data }
}
