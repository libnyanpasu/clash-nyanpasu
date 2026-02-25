import { unwrapResult } from '@/utils'
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'
import { SERVICE_PROMPT_QUERY_KEY } from './consts'

export const useServicePrompt = () => {
  const query = useQuery({
    queryKey: [SERVICE_PROMPT_QUERY_KEY],
    queryFn: async () => {
      return unwrapResult(await commands.getServiceInstallPrompt())
    },
  })

  return {
    ...query,
  }
}
