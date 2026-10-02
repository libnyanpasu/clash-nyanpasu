import { useBlockTask } from '@/components/providers/block-task-provider'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  ClashProxiesProviderQueryItem,
  useUpdateClashProxiesProvider,
} from '@nyanpasu/query'

export const useProxiesProviderUpdate = (
  data: ClashProxiesProviderQueryItem,
) => {
  const update = useUpdateClashProxiesProvider()

  const blockTask = useBlockTask(
    `update-proxies-provider-${data.name}`,
    async () => {
      try {
        await update.mutateAsync(data.name)
      } catch (error) {
        console.error('Failed to update proxies provider', error)
        message(`Update provider failed: \n ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
          error,
        })
      }
    },
  )

  return blockTask
}
