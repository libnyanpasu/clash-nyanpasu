import { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { unwrapResult } from '@nyanpasu/rpc'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'

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
