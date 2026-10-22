import { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useActiveProfile } from '../../_modules/use-active-profile'

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
