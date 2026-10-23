import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { ask, message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useProfileMutations } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'

export const useDeleteProfile = (
  profile: ProfileItem_Serialize,
  options?: {
    /** Replaces the generic confirmation text. */
    description?: string
    onSuccess?: () => void | Promise<void>
  },
) => {
  const { drop } = useProfileMutations()

  const blockTask = useBlockTask(`delete-profile-${profile.uid}`, async () => {
    try {
      await drop.mutateAsync(profile.uid)
      await options?.onSuccess?.()
    } catch (error) {
      message(m.profile_delete_failed({ error: formatError(error) }), {
        title: m.profile_error_title(),
        kind: 'error',
        error,
      })
    }
  })

  const handleClick = useLockFn(async () => {
    const answer = await ask(
      options?.description ?? m.profile_delete_description(),
      {
        title: m.profile_delete_title(),
        kind: 'warning',
      },
    )

    // user cancelled the deletion
    if (!answer) {
      return
    }

    await blockTask.execute()
  })

  return {
    handleClick,
    isPending: blockTask.isPending,
  }
}
