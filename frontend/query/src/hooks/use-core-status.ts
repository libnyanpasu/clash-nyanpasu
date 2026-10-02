import { unwrapResult } from '@nyanpasu/rpc'
import { useQuery } from '@tanstack/react-query'
import { invokeQuery } from '../ipc/query-options'
import { useQueryApi } from '../provider/rpc-provider'

export function useCoreStatus() {
  const api = useQueryApi()
  const statusOptions = api.queries.getCoreStatus()
  const query = useQuery({
    queryKey: statusOptions.queryKey,
    queryFn: async () => {
      const res = await invokeQuery(statusOptions)

      const result = unwrapResult(res)

      if (!result) {
        return null
      }

      const status =
        result.state && 'Running' in result.state ? 'Running' : result.state

      return {
        status,
        startAt: result.state_changed_at,
        type: result.host,
        controller:
          status === 'Running' && result.connectivity.kind === 'connected'
            ? result.controller
            : null,
      }
    },
  })

  return query
}
