import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import {
  ClashProxiesProviderQueryItem,
  invokeMutation,
  useQueryApi,
} from '@nyanpasu/query'
import { unwrapResult } from '@nyanpasu/rpc'
import { useQueryClient } from '@tanstack/react-query'

export const useProxiesProviderHealthcheck = (
  data: ClashProxiesProviderQueryItem,
) => {
  const api = useQueryApi()
  const queryClient = useQueryClient()

  const blockTask = useBlockTask(
    `healthcheck-proxies-provider-${data.name}`,
    async () => {
      try {
        unwrapResult(
          await invokeMutation(api.mutations.clashApiHealthcheckProxyProvider, [
            data.name,
          ]),
        )

        await Promise.all([
          queryClient.invalidateQueries({
            queryKey: api.queries.clashApiGetProvidersProxies().queryKey,
          }),
          queryClient.invalidateQueries({
            queryKey: api.queries.getProxies().queryKey,
          }),
        ])
      } catch (error) {
        console.error('Failed to health check proxies provider', error)
        message(m.providers_healthcheck_failed_message({ name: data.name }), {
          kind: 'error',
          error,
        })
      }
    },
  )

  return blockTask
}
