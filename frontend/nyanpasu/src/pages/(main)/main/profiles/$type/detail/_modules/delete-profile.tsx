import { ComponentProps } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Profile, useProfile } from '@nyanpasu/interface'
import { useNavigate } from '@tanstack/react-router'
import { ask } from '@tauri-apps/plugin-dialog'
import { Route as IndexRoute } from '../$uid'

export const useDeleteProfile = (
  profile: Profile,
  options?: {
    onSuccess?: () => void | Promise<void>
  },
) => {
  const { drop } = useProfile()

  const blockTask = useBlockTask(`delete-profile-${profile.uid}`, async () => {
    try {
      await drop.mutateAsync(profile.uid)
      await options?.onSuccess?.()
    } catch (error) {
      message(`Delete failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
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
  profile: Profile
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
