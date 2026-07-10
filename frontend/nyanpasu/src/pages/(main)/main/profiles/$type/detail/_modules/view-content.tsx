import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { commands, type ProfileItem_Serialize } from '@nyanpasu/interface'

export default function ViewContent({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: ProfileItem_Serialize
}) {
  const handleClick = useLockFn(async () => {
    await commands.createEditorWindow('profile', profile.uid)
  })

  return <Button {...props} onClick={handleClick} />
}
