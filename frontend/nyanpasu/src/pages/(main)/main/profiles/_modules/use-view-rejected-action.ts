import { m } from '@/paraglide/messages'
import { commands } from '@/services/rpc'
import { rejectedSnapshotId } from '@/utils/ipc-error'
import type { MessageOptions } from '@/utils/notification'
import { isTauri } from '@nyanpasu/platform'
import { unwrapResult } from '@nyanpasu/rpc'

export function useViewRejectedAction() {
  return (error: unknown): MessageOptions['action'] => {
    if (!isTauri()) return undefined
    const id = rejectedSnapshotId(error)
    if (!id) return undefined

    return {
      label: m.error_runtime_core_rejected_config_view(),
      onSelect: async () => {
        unwrapResult(await commands.openRejectedConfigWindow(id))
      },
    }
  }
}
