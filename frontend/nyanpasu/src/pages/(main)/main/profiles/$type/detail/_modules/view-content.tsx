import { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'

export default function ViewContent({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: ProfileItem_Serialize
}) {
  const handleClick = useLockFn(async () => {
    await rpc.createEditorWindow('profile', profile.uid)
  })

  return <Button {...props} onClick={handleClick} />
}
