import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { commands, Profile } from '@nyanpasu/interface'

export default function ViewContent({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: Profile
}) {
  const handleClick = useLockFn(async () => {
    await commands.createEditorWindow(profile.uid)
  })

  return <Button {...props} onClick={handleClick} />
}
