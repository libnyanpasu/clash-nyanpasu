import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { commands, Profile, unwrapResult } from '@nyanpasu/interface'

export default function OpenLocally({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick'> & {
  profile: Profile
}) {
  const handleClick = useLockFn(async () => {
    unwrapResult(await commands.viewProfile(profile.uid))
  })

  return <Button {...props} onClick={handleClick} />
}
