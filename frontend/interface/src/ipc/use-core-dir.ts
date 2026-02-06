import { unwrapResult } from '@/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'
import { CORE_DIR_QUERY_KEY } from './consts'

export const useCoreDir = () => {
  const query = useQuery({
    queryKey: [CORE_DIR_QUERY_KEY],
    queryFn: async () => {
      return unwrapResult(await commands.getCoreDir())
    },
  })

  return {
    ...query,
  }
}
