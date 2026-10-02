import { useBlockTask } from '@/components/providers/block-task-provider'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  ClashRulesProviderQueryItem,
  useUpdateClashRulesProvider,
} from '@nyanpasu/query'

export const useRulesProviderUpdate = (data: ClashRulesProviderQueryItem) => {
  const update = useUpdateClashRulesProvider()

  const blockTask = useBlockTask(
    `update-rules-provider-${data.name}`,
    async () => {
      try {
        await update.mutateAsync(data.name)
      } catch (error) {
        console.error('Failed to update rules provider', error)
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
