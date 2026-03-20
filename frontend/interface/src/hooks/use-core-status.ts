import { commands } from '@/ipc'
import { CLASH_CORE_STATUS_QUERY_KEY } from '@/ipc/consts'
import { unwrapResult } from '@/utils'
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

      const [status, startAt, type] = result

      return {
        status,
        startAt,
        type,
      }
    },
  })

  return query
}
