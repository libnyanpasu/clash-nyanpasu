import { commands } from '@interface/ipc'
import { CLASH_CORE_STATUS_QUERY_KEY } from '@interface/ipc/consts'
import { unwrapResult } from '@interface/utils'
import { useQuery } from '@tanstack/react-query'

export function useCoreStatus() {
  const query = useQuery({
    queryKey: [CLASH_CORE_STATUS_QUERY_KEY],
    queryFn: async () => {
      const res = await commands.getCoreStatus()

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
      }
    },
  })

  return query
}
