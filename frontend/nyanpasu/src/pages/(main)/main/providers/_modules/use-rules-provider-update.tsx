import { useBlockTask } from '@/components/providers/block-task-provider'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { ClashRulesProviderQueryItem } from '@nyanpasu/interface'

export const useRulesProviderUpdate = (data: ClashRulesProviderQueryItem) => {
  const blockTask = useBlockTask(
    `update-rules-provider-${data.name}`,
    async () => {
      try {
        await data.mutate()
      } catch (error) {
        console.error('Failed to update rules provider', error)
        message(`Update provider failed: \n ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
        })
      }
    },
  )

  return blockTask
}
