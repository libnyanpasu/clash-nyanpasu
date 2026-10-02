import { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import {
  useCurrentProfileUid,
  useDeleteClashConnections,
  useProfileMutations,
} from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'

export const useActiveProfile = (profile: ProfileItem_Serialize) => {
  const { activate } = useProfileMutations()

  const isActive = useCurrentProfileUid() === profile.uid

  const deleteConnections = useDeleteClashConnections()

  const blockTask = useBlockTask(`active-profile-${profile.uid}`, async () => {
    try {
      await activate.mutateAsync(profile.uid)
    } catch (err) {
      message(
        `${m.profile_active_title_error({
          name: profile.name,
        })} \n ${formatError(err)}`,
        {
          title: 'Error',
          kind: 'error',
          error: err,
        },
      )

      return
    }

    // Legacy UX: unconditionally drop connections after activation. The
    // backend also interrupts connections (opt-in, default off); when that
    // option is enabled the double interruption is idempotent. A core with
    // no usable API (e.g. in an error state) has nothing to drop, so a
    // failure here does not fail the activation.
    try {
      await deleteConnections.mutateAsync(null)
    } catch (err) {
      console.warn('[active-profile] failed to delete connections:', err)
    }

    message(m.profile_active_title_success({ name: profile.name }), {
      title: m.profile_active_title(),
      kind: 'info',
    })
  })

  const handleClick = useLockFn(async () => {
    if (isActive) {
      message(m.profile_is_active_description(), {
        title: m.profile_active_title(),
        kind: 'info',
      })

      return
    }

    await blockTask.execute()
  })

  return {
    isActive,
    handleClick,
    isPending: blockTask.isPending,
  }
}

export default function ActiveButton({
  profile,
  className,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: ProfileItem_Serialize
}) {
  const { isActive, handleClick, isPending } = useActiveProfile(profile)

  return (
    <Button
      {...props}
      className={cn(
        'transition-colors',
        className,
        isActive && [
          'bg-green-500/30 text-green-900 hover:bg-green-500/50',
          'dark:bg-green-900/50 dark:text-green-600 dark:hover:bg-green-900/60',
        ],
      )}
      onClick={handleClick}
      loading={isPending}
    />
  )
}
