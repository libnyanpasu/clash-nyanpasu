import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useProfileMutations } from '@nyanpasu/query'
import { type RemoteProfileOptionsPatch_Deserialize } from '@nyanpasu/rpc/types'

/** A patch that leaves every remote option unchanged. */
export const UNCHANGED_REMOTE_OPTIONS = {
  with_proxy: null,
  self_proxy: null,
  update_interval_minutes: null,
} as const satisfies RemoteProfileOptionsPatch_Deserialize

/**
 * Patches the remote options of a profile. Each edited field has its own task
 * key, so changing one field never waits on another. Resolves to whether the
 * patch was applied; a failure is already reported to the user.
 */
export function useRemoteOptionsPatch(uid: string, field: string) {
  const { patchRemoteOptions } = useProfileMutations()

  const task = useBlockTask(
    `update-remote-profile-options-${uid}-${field}`,
    async (patch: RemoteProfileOptionsPatch_Deserialize) => {
      try {
        await patchRemoteOptions.mutateAsync({ uid, patch })

        return true
      } catch (error) {
        message(m.profile_update_failed({ error: formatError(error) }), {
          title: m.profile_error_title(),
          kind: 'error',
          error,
        })

        return false
      }
    },
  )

  return { patch: task.execute, isPending: task.isPending }
}
