import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  rpc,
  unwrapResult,
  type ProfileItem_Serialize,
} from '@nyanpasu/interface'

export default function OpenLocally({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick'> & {
  profile: ProfileItem_Serialize
}) {
  const handleClick = useLockFn(async () => {
    try {
      unwrapResult(await rpc.viewProfile(profile.uid))
    } catch (error) {
      await message(
        `${m.profile_open_locally_title()}: ${formatError(error)}`,
        {
          kind: 'error',
          error,
        },
      )
    }
  })

  return <Button {...props} onClick={handleClick} />
}
