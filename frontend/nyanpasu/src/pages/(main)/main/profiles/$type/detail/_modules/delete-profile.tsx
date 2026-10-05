import { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { ask, message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useProfileMutations } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { useNavigate } from '@tanstack/react-router'
import { Route as IndexRoute } from '../$uid'

export const useDeleteProfile = (
  profile: ProfileItem_Serialize,
  options?: {
    onSuccess?: () => void | Promise<void>
  },
) => {
  const { drop } = useProfileMutations()

  const blockTask = useBlockTask(`delete-profile-${profile.uid}`, async () => {
    try {
      await drop.mutateAsync(profile.uid)
      await options?.onSuccess?.()
    } catch (error) {
      message(`Delete failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  const handleClick = useLockFn(async () => {
    const answer = await ask(m.profile_delete_description(), {
      title: m.profile_delete_title(),
      kind: 'warning',
    })

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

export default function DeleteProfile({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: ProfileItem_Serialize
}) {
  const { type } = IndexRoute.useParams()

  const navigate = useNavigate()

  const { handleClick, isPending } = useDeleteProfile(profile, {
    onSuccess: async () => {
      await navigate({
        to: `/main/profiles/$type`,
        params: { type },
      })
    },
  })

  return <Button {...props} onClick={handleClick} loading={isPending} />
}
