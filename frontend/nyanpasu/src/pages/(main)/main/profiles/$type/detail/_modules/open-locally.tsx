import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
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
    unwrapResult(await rpc.viewProfile(profile.uid))
  })

  return <Button {...props} onClick={handleClick} />
}
