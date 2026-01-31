import { ComponentProps } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Profile, useClashConnections, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

export default function ActiveButton({
  profile,
  className,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: Profile
}) {
  const {
    query: { data },
    upsert,
  } = useProfile()

  const isActive = data?.current?.find((uid) => uid === profile.uid)

  const { deleteConnections } = useClashConnections()

  const blockTask = useBlockTask(`active-profile-${profile.uid}`, async () => {
    try {
      await upsert.mutateAsync({ current: [profile.uid] })

      await deleteConnections.mutateAsync(null)

      message(m.profile_active_title_success({ name: profile.name }), {
        title: m.profile_active_title(),
        kind: 'info',
      })
    } catch (err) {
      // This FetchError was triggered by the `DELETE /connections` API
      const isFetchError = err instanceof Error && err.name === 'FetchError'

      message(
        isFetchError
          ? `Failed to delete connections: \n ${formatError(err)}`
          : `${m.profile_active_title_error({
              name: profile.name,
            })} \n ${formatError(err)}`,
        {
          title: 'Error',
          kind: isFetchError ? 'warning' : 'error',
        },
      )
    }
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
      loading={blockTask.isPending}
    />
  )
}
