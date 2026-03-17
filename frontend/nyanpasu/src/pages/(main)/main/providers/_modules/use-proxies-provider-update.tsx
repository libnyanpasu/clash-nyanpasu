import { useBlockTask } from '@/components/providers/block-task-provider'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { ClashProxiesProviderQueryItem } from '@nyanpasu/interface'

export const useProxiesProviderUpdate = (
  data: ClashProxiesProviderQueryItem,
) => {
  const blockTask = useBlockTask(
    `update-proxies-provider-${data.name}`,
    async () => {
      try {
        await data.mutate()
      } catch (error) {
        console.error('Failed to update proxies provider', error)
        message(`Update provider failed: \n ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
        })
      }
    },
  )

  return blockTask
}
